//! A quiet next-step line for a player with nothing selected, so a new
//! tribe is never left with fish piling up and no idea what to spend it on.

/// What the tribe has, as far as advice cares.
#[derive(Default, Clone, Copy)]
pub struct Standing {
    pub fish: u32,
    /// Training camps finished, and ones still going up.
    pub camps: usize,
    pub camps_going: usize,
    /// Workers with no fishing slot, when every slot the tribe has is taken.
    pub no_slot: usize,
    /// Sushi Houses and Bunkhouses, finished or going up.
    pub houses: usize,
    pub bunkhouses: usize,
}

/// The one thing worth doing next, in plain words, or nothing when the
/// tribe is doing fine. Fish comes from fishing slots, everyone else from
/// the buildings that house them, and newcomers swim in to fill them.
pub fn next_step(s: &Standing) -> Option<&'static str> {
    if s.houses == 0 {
        return Some("Build a Sushi House by the water (B, F): its fishers bring in the fish that pays for everything. With nobody idle, newcomers swim in from the sea to fill it.");
    }
    if s.bunkhouses == 0 {
        return Some("Build a Bunkhouse (B, R): each bunk drafts a worker into a Snowballer, and your Chieftain leads them.");
    }
    if s.no_slot >= 3 {
        return Some("Workers are standing idle: want more slots filled (roll the mouse wheel over a building's slots), or build a Sushi House, Bunkhouse or camp for them.");
    }
    if s.camps + s.camps_going == 0 {
        return Some("Open Camps in the build menu: each camp slot takes a worker, who mines a crystal and trains as a specialist.");
    }
    if s.fish >= 60 {
        return Some("Fish is piling up: build a Bunkhouse or camp up for more room, and want its new slots filled.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_tribe_is_told_to_build_a_sushi_house_then_a_bunkhouse() {
        let s = Standing::default();
        assert!(next_step(&s).unwrap().contains("Sushi House"));
        let s = Standing { houses: 1, ..s };
        assert!(next_step(&s).unwrap().contains("Bunkhouse"));
    }

    #[test]
    fn idle_workers_with_full_slots_say_to_want_more_slots() {
        let s = Standing { no_slot: 4, houses: 1, bunkhouses: 1, ..Standing::default() };
        assert!(next_step(&s).unwrap().contains("idle"));
        assert!(!next_step(&Standing { no_slot: 1, ..s }).unwrap_or("").contains("idle"));
    }

    fn tribe() -> Standing {
        Standing { fish: 13, houses: 1, bunkhouses: 1, ..Standing::default() }
    }

    #[test]
    fn nobody_is_told_to_build_a_range_or_draft_by_hand() {
        let says = next_step(&tribe()).unwrap();
        assert!(says.contains("Camps") && !says.contains("Range") && !says.contains("press"), "{says}");
    }

    #[test]
    fn nobody_is_told_to_chop() {
        // Builders fetch their own logs; there is no woodpile to fill.
        assert!(!next_step(&tribe()).unwrap_or("").contains("chop"));
    }

    #[test]
    fn a_tribe_that_has_it_all_is_left_alone() {
        let s = Standing { camps: 2, fish: 10, ..tribe() };
        assert_eq!(next_step(&s), None);
    }
}
