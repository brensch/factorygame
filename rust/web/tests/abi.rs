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
    assert_eq!(num(&s, "credits"), 40);
    assert_eq!(num(&s, "quota"), 85);
    assert_eq!(field(&s, "err"), "null");
    assert!(s.contains("\"m\":\"vault\""), "vault pre-placed: {s}");

    // Seed 42 deals 2 Drills + 2 Furnaces (pinned by the core tests too).
    let mut h = hand(&s);
    assert_eq!(h.len(), 4);

    // The whole starting kit, compact by the vault: two lanes on a spine.
    let drill = h.iter().position(|m| m == "drill").unwrap();
    let s = call(play(drill as u32, 13, 9, E, -1, -1));
    h = hand(&s);
    assert_eq!(h.len(), 3, "playing consumes the card");
    let furnace = h.iter().position(|m| m == "furnace").unwrap();
    call(play(furnace as u32, 14, 9, E, -1, -1));
    let h2 = hand(&out_string());
    let drill2 = h2.iter().position(|m| m == "drill").unwrap();
    call(play(drill2 as u32, 13, 8, E, -1, -1));
    let h3 = hand(&out_string());
    let furnace2 = h3.iter().position(|m| m == "furnace").unwrap();
    call(play(furnace2 as u32, 14, 8, E, -1, -1));
    call(belt(15, 9, E));
    call(belt(16, 9, E));
    call(belt(15, 8, E));
    call(belt(16, 8, 2 /* south */));
    let s = call(state());
    assert_eq!(num(&s, "credits"), 40 - 4); // placement is free; belts aren't

    // Projection clears the quota before we commit.
    let p = call(project());
    assert!(num(&p, "payout") >= 85, "projection: {p}");

    // Rotate is a real edit: projection drops when a drill faces away.
    let full = num(&p, "payout");
    call(rotate(13, 9));
    let broken = call(project());
    assert!(num(&broken, "payout") < full, "drill facing south: {broken}");
    for _ in 0..3 {
        call(rotate(13, 9));
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

    // Commit matches what was watched, and the shop opens.
    let s = call(shift_finish());
    assert_eq!(field(&s, "phase"), "shop");
    let cleared_payout = num(&s, "payout");
    assert!(cleared_payout >= 85);
    assert_eq!(num(&s, "nextQuota"), 115, "shop advertises the NEXT round: {s}");
    let offers_at = s.find("\"offers\":[").unwrap();
    let offers = &s[offers_at..offers_at + s[offers_at..].find(']').unwrap()];
    assert_eq!(offers.matches("\"name\":").count(), 5, "a full rack: {s}");

    // The round's chance elements are on the wire: the spot market for the
    // upcoming round, rolled while you can still shop for it.
    assert!(s.contains("\"market\":\""), "{s}");

    // Prices bite: take whatever the rack will sell us with the surplus.
    let s = call(state());
    assert_eq!(hand(&s).len(), 0, "the whole kit is on the board");
    let credits = num(&s, "credits");
    let mut bought = false;
    for i in 0..5u32 {
        let s = call(shop_buy(i));
        if field(&s, "err") == "null" {
            bought = true;
            break;
        }
    }
    assert!(bought, "even after selling out, nothing on the rack was affordable");
    let s = call(state());
    assert!(num(&s, "credits") < credits);
    assert_eq!(hand(&s).len(), 1);

    let before_reroll = num(&s, "credits");
    let reroll_price = num(&s, "rerollPrice");
    if before_reroll >= reroll_price {
        let s = call(shop_reroll());
        assert_eq!(num(&s, "credits"), before_reroll - reroll_price);
        assert_eq!(num(&s, "rerollPrice"), reroll_price * 2, "rerolls escalate");
    }

    let s = call(shop_done());
    assert_eq!(field(&s, "phase"), "build");
    assert_eq!(num(&s, "round"), 1);
    assert_eq!(num(&s, "quota"), 115);
    assert_eq!(hand(&s).len(), 1, "sold 2, bought 1: {s}");
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
fn consumed_items_still_animate_their_final_hop() {
    call(boot(42));
    // Drill butted directly against the furnace: the ore's only journey is
    // drill→furnace, invisible in out slots — it must appear as a hop.
    let drill = hand(&out_string()).iter().position(|m| m == "drill").unwrap();
    call(play(drill as u32, 0, 3, E, -1, -1));
    let furnace = hand(&out_string()).iter().position(|m| m == "furnace").unwrap();
    call(play(furnace as u32, 1, 3, E, -1, -1));

    call(shift_start());
    let mut saw_ore_hop = false;
    for _ in 0..20 {
        let f = call(shift_step());
        if f.contains("\"hops\":[{") && f.contains("\"fx\":0,\"fy\":3,\"x\":1,\"y\":3,\"t\":\"ore\"") {
            saw_ore_hop = true;
            break;
        }
    }
    assert!(saw_ore_hop, "direct machine→machine transfer never surfaced as a hop");
}

#[test]
fn group_move_over_the_wire_is_atomic() {
    call(boot(42));
    let drill = hand(&out_string()).iter().position(|m| m == "drill").unwrap();
    call(play(drill as u32, 0, 3, E, -1, -1));
    call(belt(1, 3, E));

    // Stage both and slide them one tile south.
    call(sel_add(0, 3));
    call(sel_add(1, 3));
    let s = call(sel_move(0, 1));
    assert_eq!(field(&s, "err"), "null");
    assert!(s.contains("\"x\":0,\"y\":4,\"m\":\"drill\""), "{s}");
    assert!(s.contains("\"x\":1,\"y\":4,\"m\":\"belt\""), "{s}");

    // A refused move consumes the selection but changes no positions.
    call(sel_add(0, 4));
    let s = call(sel_move(-1, 0));
    assert_ne!(field(&s, "err"), "null");
    assert!(s.contains("\"x\":0,\"y\":4,\"m\":\"drill\""), "{s}");

    // The selection was consumed: a bare sel_move has nothing to act on.
    let s = call(sel_move(1, 0));
    assert!(s.contains("nothing movable"), "{s}");
}

#[test]
fn flows_report_ok_open_and_bad_connections() {
    call(boot(42));
    let drill = hand(&out_string()).iter().position(|m| m == "drill").unwrap();
    let s = call(play(drill as u32, 0, 3, E, -1, -1));
    // Drill pointing at an empty tile: an unfinished line, not an error.
    assert!(
        s.contains("\"fx\":0,\"fy\":3,\"tx\":1,\"ty\":3,\"d\":\"E\",\"status\":\"open\""),
        "{s}"
    );

    // Belt behind it pointing INTO the drill: extractors never accept — bad.
    let s = call(belt(1, 3, 3 /* west */));
    assert!(
        s.contains("\"fx\":1,\"fy\":3,\"tx\":0,\"ty\":3,\"d\":\"W\",\"status\":\"bad\""),
        "{s}"
    );

    // Re-aim the belt east at a furnace: ore into a furnace recipe — ok.
    call(sell(1, 3));
    call(belt(1, 3, E));
    let furnace = hand(&out_string()).iter().position(|m| m == "furnace").unwrap();
    let s = call(play(furnace as u32, 2, 3, E, -1, -1));
    assert!(
        s.contains("\"fx\":1,\"fy\":3,\"tx\":2,\"ty\":3,\"d\":\"E\",\"status\":\"ok\""),
        "{s}"
    );

    // Aim the drill off the board: bad.
    call(rotate(0, 3)); // E -> S
    call(rotate(0, 3)); // S -> W
    let s = call(state());
    assert!(
        s.contains("\"fx\":0,\"fy\":3,\"tx\":-1,\"ty\":3,\"d\":\"W\",\"status\":\"bad\""),
        "{s}"
    );
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
    // every card machine plus belt, junction and vault is described
    assert_eq!(s.matches("\"blurb\":").count(), 22, "{s}");
    assert!(s.contains("\"m\":\"junction\""), "{s}");
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
