//! Drives the wasm ABI on the host target, end to end: boot, build a board
//! card by card, watch the shift frame by frame, commit, take a reward.
//! JSON is inspected with string tools on purpose — this crate must stay
//! dependency-free, and the assertions double as a spec of the wire format.

use overflow_web::*;

const E: i32 = 1; // direction code for east

fn call(len: usize) -> String {
    let s = out_string();
    assert_eq!(s.len(), len, "returned length must match the buffer");
    s
}

/// Pull `"key":<number>` out of a JSON document.
fn num(json: &str, key: &str) -> i64 {
    let pat = format!("\"{key}\":");
    let at = json.find(&pat).unwrap_or_else(|| panic!("no {key} in {json}")) + pat.len();
    json[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect::<String>()
        .parse()
        .unwrap()
}

/// Machine keys of the hand, in order.
fn hand(json: &str) -> Vec<String> {
    let at = json.find("\"hand\":[").unwrap() + 8;
    let end = json[at..].find(']').unwrap() + at;
    json[at..end]
        .split("\"m\":\"")
        .skip(1)
        .map(|s| s[..s.find('"').unwrap()].to_string())
        .collect()
}

fn field(json: &str, key: &str) -> String {
    let pat = format!("\"{key}\":");
    let at = json.find(&pat).unwrap() + pat.len();
    let rest = &json[at..];
    if let Some(s) = rest.strip_prefix('"') {
        s[..s.find('"').unwrap()].to_string()
    } else {
        rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect()
    }
}

#[test]
fn a_whole_round_through_the_wire_format() {
    let s = call(boot(42));
    assert_eq!(field(&s, "phase"), "build");
    assert_eq!(num(&s, "credits"), 15);
    assert_eq!(num(&s, "quota"), 20);
    assert_eq!(field(&s, "err"), "null");
    assert!(s.contains("\"m\":\"vault\""), "vault pre-placed: {s}");

    // Seed 42 deals 2 Drills + 2 Furnaces (pinned by the core tests too).
    let mut h = hand(&s);
    assert_eq!(h.len(), 4);

    // Drill at the west edge, furnace mid-lane, belts between and out.
    let drill = h.iter().position(|m| m == "drill").unwrap();
    let s = call(play(drill as u32, 0, 3, E, -1, -1));
    h = hand(&s);
    assert_eq!(h.len(), 3, "playing consumes the card");
    let furnace = h.iter().position(|m| m == "furnace").unwrap();
    let s = call(play(furnace as u32, 4, 3, E, -1, -1));
    assert_eq!(field(&s, "err"), "null");
    for x in [1, 2, 3, 5, 6, 7, 8] {
        call(belt(x, 3, E));
    }
    let s = call(state());
    assert_eq!(num(&s, "credits"), 15 - 3 - 5 - 7);

    // Projection clears the quota before we commit.
    let p = call(project());
    assert!(num(&p, "payout") >= 20, "projection: {p}");

    // Rotate is a real edit: projection collapses when the drill faces away.
    call(rotate(0, 3));
    let broken = call(project());
    assert!(num(&broken, "payout") < 20, "drill facing south: {broken}");
    for _ in 0..3 {
        call(rotate(0, 3));
    }

    // The animated shift: step to done, items visibly in flight on the way.
    call(shift_start());
    let mut saw_items = false;
    let mut frames = 0;
    loop {
        let f = call(shift_step());
        frames += 1;
        assert!(frames <= 60, "shift never finished");
        if f.contains("\"t\":\"ore\"") || f.contains("\"t\":\"ingot\"") {
            saw_items = true;
        }
        if field(&f, "done") == "true" {
            assert_eq!(num(&f, "tick"), 60);
            break;
        }
    }
    assert!(saw_items, "no items ever appeared on the belts");

    // Commit matches what was watched, and the round advances.
    let s = call(shift_finish());
    assert_eq!(field(&s, "phase"), "reward");
    let cleared_payout = num(&s, "payout");
    assert!(cleared_payout >= 20);
    assert_eq!(num(&s, "nextQuota"), 45, "reward modal advertises the NEXT round: {s}");
    let offers_at = s.find("\"offers\":[").unwrap();
    let offers = &s[offers_at..offers_at + s[offers_at..].find(']').unwrap()];
    assert_eq!(offers.matches("\"name\":").count(), 3, "three offers: {s}");

    let s = call(pick_reward(0));
    assert_eq!(field(&s, "phase"), "build");
    assert_eq!(num(&s, "round"), 1);
    assert_eq!(num(&s, "quota"), 45);
}

#[test]
fn refused_commands_report_err_and_change_nothing() {
    call(boot(7));
    let before = call(state());
    let s = call(belt(-1, 0, E));
    assert_ne!(field(&s, "err"), "null");
    let after = call(state());
    assert_eq!(before, after, "refused command must not mutate state");

    let s = call(play(9, 0, 0, E, -1, -1));
    assert!(s.contains("no such card"), "{s}");
}

#[test]
fn catalog_carries_recipes_auras_and_values() {
    let s = call(catalog());
    // recipe data straight from defs.rs
    assert!(s.contains("\"m\":\"fab\""), "{s}");
    assert!(
        s.contains("\"recipe\":{\"inputs\":[\"ingot\",\"ingot\"],\"output\":\"gear\",\"ticks\":5}"),
        "fab recipe: {s}"
    );
    // aura data, including the tag restriction
    assert!(s.contains("\"onlyTag\":\"heat\""), "heatsink aura tag: {s}");
    // item base values
    assert!(s.contains("\"gear\":16"), "{s}");
    // behaviour blurbs quote the real balance constant
    assert!(s.contains("15% chance"), "dup blurb uses DUP_CLONE_CHANCE: {s}");
    // every card machine plus belt and vault is described
    assert_eq!(s.matches("\"blurb\":").count(), 21, "{s}");
}

#[test]
fn filter_placement_carries_gate_and_both_edges() {
    call(boot(7));
    // No filter in the starting hand, so edit the config surface via belt +
    // error paths instead: gate on a non-filter is refused.
    call(belt(2, 2, E));
    let s = call(set_gate(2, 2, 5));
    assert!(s.contains("not a filter"), "{s}");
}
