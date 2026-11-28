#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum FaceKind {
    Strike,
    Block,
    Blank,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Die {
    faces: Vec<FaceKind>,
}

impl Die {
    pub fn new(faces: Vec<FaceKind>) -> Option<Self> {
        (1..=64).contains(&faces.len()).then_some(Self { faces })
    }

    pub fn faces(&self) -> &[FaceKind] {
        &self.faces
    }
}

impl Default for Die {
    fn default() -> Self {
        Self {
            faces: vec![
                FaceKind::Strike,
                FaceKind::Strike,
                FaceKind::Strike,
                FaceKind::Block,
                FaceKind::Block,
                FaceKind::Blank,
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiceCount(u16);

impl DiceCount {
    pub fn new(value: u16) -> Option<Self> {
        (value <= 1000).then_some(Self(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamagePerStrike(u8);

impl DamagePerStrike {
    pub fn new(value: u8) -> Option<Self> {
        (value <= 100).then_some(Self(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackOutcome {
    pub attack_faces: Vec<u8>,
    pub defense_faces: Vec<u8>,
    pub strikes: u16,
    pub blocks: u16,
    pub unblocked_strikes: u16,
    pub damage: u32,
}

pub fn draw(seed: u64, n: u64) -> u64 {
    let mut z = seed.wrapping_add(n.wrapping_add(1).wrapping_mul(0x9E3779B97F4A7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn roll_with_source(draws_used: &mut u64, die: &Die, mut source: impl FnMut(u64) -> u64) -> u8 {
    let faces = die.faces.len() as u64;
    let rem = (u64::MAX % faces + 1) % faces;
    loop {
        let value = source(*draws_used);
        *draws_used = draws_used.wrapping_add(1);
        if value <= u64::MAX - rem {
            return (value % faces) as u8;
        }
    }
}

pub fn roll_die(seed: u64, draws_used: &mut u64, die: &Die) -> u8 {
    roll_with_source(draws_used, die, |n| draw(seed, n))
}

pub fn resolve_attack(
    attack_dice: DiceCount,
    defense_dice: DiceCount,
    damage_per_strike: DamagePerStrike,
    die: &Die,
    seed: u64,
    mut draws_used: u64,
) -> (AttackOutcome, u64) {
    let attack_faces: Vec<_> = (0..attack_dice.0)
        .map(|_| roll_die(seed, &mut draws_used, die))
        .collect();
    let defense_faces: Vec<_> = (0..defense_dice.0)
        .map(|_| roll_die(seed, &mut draws_used, die))
        .collect();
    let strikes = attack_faces
        .iter()
        .filter(|&&face| die.faces[face as usize] == FaceKind::Strike)
        .count() as u16;
    let blocks = defense_faces
        .iter()
        .filter(|&&face| die.faces[face as usize] == FaceKind::Block)
        .count() as u16;
    let unblocked_strikes = strikes.saturating_sub(blocks);
    let damage = u32::from(unblocked_strikes) * u32::from(damage_per_strike.0);
    (
        AttackOutcome {
            attack_faces,
            defense_faces,
            strikes,
            blocks,
            unblocked_strikes,
            damage,
        },
        draws_used,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(value: u16) -> DiceCount {
        DiceCount::new(value).unwrap()
    }

    fn damage(value: u8) -> DamagePerStrike {
        DamagePerStrike::new(value).unwrap()
    }

    #[test]
    fn splitmix_known_answers() {
        for (seed, expected) in [
            (
                0,
                [0xe220a8397b1dcdaf, 0x6e789e6aa1b965f4, 0x06c45d188009454f],
            ),
            (
                0x0123456789abcdef,
                [0x157a3807a48faa9d, 0xd573529b34a1d093, 0x2f90b72e996dccbe],
            ),
        ] {
            for (n, value) in expected.into_iter().enumerate() {
                assert_eq!(draw(seed, n as u64), value);
            }
        }
    }

    #[test]
    fn worked_attack_example() {
        let (outcome, draws_used) = resolve_attack(
            count(3),
            count(2),
            damage(1),
            &Die::default(),
            0x0123456789abcdef,
            0,
        );
        assert_eq!(outcome.attack_faces, [3, 5, 2]);
        assert_eq!(outcome.defense_faces, [2, 2]);
        assert_eq!(
            (
                outcome.strikes,
                outcome.blocks,
                outcome.unblocked_strikes,
                outcome.damage
            ),
            (1, 0, 1, 1)
        );
        assert_eq!(draws_used, 5);
    }

    #[test]
    fn rejection_sampling_consumes_rejected_draws() {
        let die = Die::default();
        let mut draws_used = 7;
        let mut positions = Vec::new();
        let face = roll_with_source(&mut draws_used, &die, |n| {
            positions.push(n);
            match n {
                7 => u64::MAX,
                8 => u64::MAX - 4,
                _ => panic!("unexpected draw"),
            }
        });
        assert_eq!(positions, [7, 8]);
        assert_eq!(face, ((u64::MAX - 4) % 6) as u8);
        assert_eq!(draws_used, 9);
    }

    #[test]
    fn zero_dice_consume_no_draws() {
        let (outcome, draws_used) =
            resolve_attack(count(0), count(0), damage(100), &Die::default(), 0, 42);
        assert!(outcome.attack_faces.is_empty());
        assert!(outcome.defense_faces.is_empty());
        assert_eq!(outcome.damage, 0);
        assert_eq!(draws_used, 42);
    }

    #[test]
    fn blocks_floor_damage_at_zero() {
        let die = Die::new(vec![FaceKind::Block]).unwrap();
        let (outcome, draws_used) = resolve_attack(count(1), count(2), damage(100), &die, 0, 0);
        assert_eq!(
            (
                outcome.strikes,
                outcome.blocks,
                outcome.unblocked_strikes,
                outcome.damage
            ),
            (0, 2, 0, 0)
        );
        assert_eq!(draws_used, 3);
    }

    #[test]
    fn blocks_reduce_positive_damage() {
        let (outcome, draws_used) = resolve_attack(
            count(3),
            count(1),
            damage(2),
            &Die::default(),
            0x0123456789abcdef,
            2,
        );
        assert_eq!(outcome.attack_faces, [2, 2, 2]);
        assert_eq!(outcome.defense_faces, [4]);
        assert_eq!(
            (
                outcome.strikes,
                outcome.blocks,
                outcome.unblocked_strikes,
                outcome.damage
            ),
            (3, 1, 2, 4)
        );
        assert_eq!(draws_used, 6);
    }

    #[test]
    fn damage_bounds_and_thousand_dice() {
        let die = Die::new(vec![FaceKind::Strike]).unwrap();
        for (per_strike, expected) in [(0, 0), (100, 100_000)] {
            let (outcome, draws_used) =
                resolve_attack(count(1000), count(0), damage(per_strike), &die, 0, 0);
            assert_eq!(outcome.attack_faces.len(), 1000);
            assert_eq!(outcome.strikes, 1000);
            assert_eq!(outcome.damage, expected);
            assert_eq!(draws_used, 1000);
        }
    }

    #[test]
    fn inputs_enforce_design_limits() {
        assert!(Die::new(vec![]).is_none());
        assert!(Die::new(vec![FaceKind::Blank; 64]).is_some());
        assert!(Die::new(vec![FaceKind::Blank; 65]).is_none());
        assert!(DiceCount::new(1000).is_some());
        assert!(DiceCount::new(1001).is_none());
        assert!(DamagePerStrike::new(100).is_some());
        assert!(DamagePerStrike::new(101).is_none());
    }
}
