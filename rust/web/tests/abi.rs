//! Drives the wasm ABI on the host target, end to end: boot, wire the bays
//! to furnaces, watch shifts frame by frame, bank toward the quota, shop
//! for machines and shipments. JSON is inspected with string tools on
//! purpose — this crate must stay dependency-free, and the assertions
//! double as a spec of the wire format.

use overflow_web::*;

const E: i32 = 1; // direction code for east
const S: i32 = 2;

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

/// The starter build over the wire: a furnace beside each bay, lanes east,
/// shared spine into the vault at (17,9).
fn build_starter() {
    for (row, spine_d) in [(6i32, S), (12i32, 0 /* north */)] {
        let h = hand(&out_string());
        let f = h.iter().position(|m| m == "furnace").unwrap();
        let s = call(play(f as u32, 1, row, E, -1, -1));
        assert_eq!(field(&s, "err"), "null", "{s}");
        for x in 2..=15 {
            call(belt(x, row, E));
        }
        call(belt(16, row, spine_d));
        let range: Vec<i32> =
            if row < 9 { (row + 1..9).collect() } else { (10..row).rev().collect() };
        for yy in range {
            call(belt(16, yy, spine_d));
        }
    }
    call(belt(16, 9, E));
}

/// Animate a full shift and commit it; returns the post-commit state.
fn run_one_shift() -> String {
    let s = call(shift_start());
    assert_eq!(field(&s, "err"), "null", "{s}");
    let mut frames = 0;
    loop {
        let f = call(shift_step());
        frames += 1;
        assert!(frames <= 60, "shift never finished");
        if field(&f, "done") == "true" {
            break;
        }
    }
    call(shift_finish())
}

#[test]
fn a_whole_round_through_the_wire_format() {
    let s = call(boot(42));
    assert_eq!(field(&s, "phase"), "build");
    assert_eq!(num(&s, "credits"), 75);
    assert_eq!(num(&s, "quota"), 130);
    assert_eq!(field(&s, "err"), "null");
    assert!(s.contains("\"m\":\"vault\""), "vault pre-placed: {s}");
    assert!(s.contains("\"m\":\"bay\""), "bays pre-placed: {s}");
    assert_eq!(hand(&s).len(), 4);

    // The starter consignment waits at the docks.
    let bays_at = s.find("\"bays\":[").unwrap();
    let bays = &s[bays_at..bays_at + s[bays_at..].find("],\"lotOffers\"").unwrap()];
    assert!(bays.contains("\"total\":70"), "{bays}");
    assert!(bays.contains("\"total\":50"), "{bays}");

    build_starter();

    // Shift 1 (there is no projection — you find out by running).
    let s = run_one_shift();
    assert_eq!(field(&s, "phase"), "build", "one shift does not clear round 1");
    assert_eq!(num(&s, "shiftsUsed"), 1);
    assert!(num(&s, "roundDelivered") > 0, "shift 1 banked something: {s}");
    assert!(num(&s, "carry") > 0, "material still in the pipes");

    // Shift 2: the warm factory keeps going and clears the quota.
    let s = run_one_shift();
    assert_eq!(field(&s, "phase"), "shop", "two shifts clear round 1: {s}");
    assert!(num(&s, "roundDelivered") >= 130, "the round total stays visible in the shop");

    // The shop: 5 offers (machines + directive + contract) and 3 shipments.
    let offers_at = s.find("\"offers\":[").unwrap();
    let offers = &s[offers_at..offers_at + s[offers_at..].find(']').unwrap()];
    assert_eq!(offers.matches("\"name\":").count(), 5, "{offers}");
    assert!(offers.contains("\"type\":\"directive\""));
    assert!(offers.contains("\"type\":\"contract\""));
    let lots_at = s.find("\"lotOffers\":[").unwrap();
    let lots = &s[lots_at..lots_at + s[lots_at..].find("],\"contracts\"").unwrap()];
    assert_eq!(lots.matches("\"name\":").count(), 3, "{lots}");

    // Buy the first shipment to bay 1: credits drop, its queue grows.
    let credits = num(&s, "credits");
    let s = call(buy_lot(0, 1));
    assert_eq!(field(&s, "err"), "null", "{s}");
    assert!(num(&s, "credits") < credits);

    let s = call(shop_done());
    assert_eq!(field(&s, "phase"), "build");
    assert_eq!(num(&s, "round"), 1);
    assert_eq!(num(&s, "quota"), 175);
}

#[test]
fn failing_all_shifts_offers_retry_and_retry_rewinds() {
    call(boot(7)); // idle factory: no processing at all
    for _ in 0..3 {
        run_one_shift();
    }
    let s = call(state());
    assert_eq!(field(&s, "phase"), "over");
    let s = call(retry());
    assert_eq!(field(&s, "phase"), "build");
    assert_eq!(num(&s, "shiftsUsed"), 0);
    let bays_at = s.find("\"bays\":[").unwrap();
    assert!(s[bays_at..].contains("\"total\":70"), "queues rewound: {s}");
}

#[test]
fn consumed_items_still_animate_their_final_hop() {
    call(boot(42));
    // Furnace butted directly against bay A: the ore's only journey is
    // bay→furnace, invisible in out slots — it must appear as a hop.
    let f = hand(&out_string()).iter().position(|m| m == "furnace").unwrap();
    call(play(f as u32, 1, 6, E, -1, -1));

    call(shift_start());
    let mut saw_ore_hop = false;
    for _ in 0..20 {
        let f = call(shift_step());
        if f.contains("\"hops\":[{") && f.contains("\"fx\":0,\"fy\":6,\"x\":1,\"y\":6,\"t\":\"ore\"")
        {
            saw_ore_hop = true;
            break;
        }
    }
    assert!(saw_ore_hop, "direct bay→furnace transfer never surfaced as a hop");
}

#[test]
fn group_move_over_the_wire_is_atomic() {
    call(boot(42));
    let f = hand(&out_string()).iter().position(|m| m == "furnace").unwrap();
    call(play(f as u32, 5, 3, E, -1, -1));
    call(belt(6, 3, E));

    call(sel_add(5, 3));
    call(sel_add(6, 3));
    let s = call(sel_move(0, 1));
    assert_eq!(field(&s, "err"), "null");
    assert!(s.contains("\"x\":5,\"y\":4,\"m\":\"furnace\""), "{s}");
    assert!(s.contains("\"x\":6,\"y\":4,\"m\":\"belt\""), "{s}");

    // A refused move consumes the selection but changes no positions.
    call(sel_add(5, 4));
    let s = call(sel_move(-10, 0)); // off the west edge
    assert_ne!(field(&s, "err"), "null");
    assert!(s.contains("\"x\":5,\"y\":4,\"m\":\"furnace\""), "{s}");
}

#[test]
fn flows_report_ok_open_and_bad_connections() {
    call(boot(42));
    let f = hand(&out_string()).iter().position(|m| m == "furnace").unwrap();
    let s = call(play(f as u32, 3, 3, E, -1, -1));
    // Furnace pointing at an empty tile: an unfinished line, not an error.
    assert!(
        s.contains("\"fx\":3,\"fy\":3,\"tx\":4,\"ty\":3,\"d\":\"E\",\"status\":\"open\""),
        "{s}"
    );

    // A belt pointing INTO a bay: bays never accept — bad.
    let s = call(belt(1, 6, 3 /* west */));
    assert!(
        s.contains("\"fx\":1,\"fy\":6,\"tx\":0,\"ty\":6,\"d\":\"W\",\"status\":\"bad\""),
        "{s}"
    );

    // Belt into the furnace: recipe machines accept ore-bearing lines — ok.
    call(sell(1, 6));
    let s = call(belt(2, 3, E));
    assert!(
        s.contains("\"fx\":2,\"fy\":3,\"tx\":3,\"ty\":3,\"d\":\"E\",\"status\":\"ok\""),
        "belt into furnace: {s}"
    );
}

#[test]
fn filters_gate_by_type_over_the_wire() {
    call(boot(42));
    // No filter in the starting hand; the command surface still validates.
    call(belt(4, 4, E));
    let s = call(set_type_gate(4, 4, 13 /* slag */));
    assert!(s.contains("not a filter"), "{s}");
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
    assert!(s.contains("\"m\":\"fab\""), "{s}");
    assert!(
        s.contains("\"recipe\":{\"inputs\":[\"ingot\",\"ingot\"],\"output\":\"gear\",\"ticks\":5}"),
        "fab recipe: {s}"
    );
    assert!(s.contains("\"onlyTag\":\"heat\""), "heatsink aura tag: {s}");
    assert!(s.contains("\"gear\":16"), "{s}");
    assert!(s.contains("15% chance"), "dup blurb uses DUP_CLONE_CHANCE: {s}");
    // card pool (14) + belt, junction, merger, splitter, chute, bay, vault
    assert_eq!(s.matches("\"blurb\":").count(), 21, "{s}");
    assert!(s.contains("\"m\":\"bay\""), "{s}");
    assert!(s.contains("\"m\":\"chute\""), "{s}");
}
