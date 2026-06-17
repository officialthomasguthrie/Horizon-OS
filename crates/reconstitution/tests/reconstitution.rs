use reconstitution::{combine, split, Error, Share};

fn secret() -> Vec<u8> {
    (0u8..32)
        .map(|i| i.wrapping_mul(7).wrapping_add(3))
        .collect()
}

// Every k-sized subset of 0..n, by index.
fn combos(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn rec(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..n {
            cur.push(i);
            rec(i + 1, n, k, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    rec(0, n, k, &mut Vec::new(), &mut out);
    out
}

fn pick(shares: &[Share], idx: &[usize]) -> Vec<Share> {
    idx.iter().map(|&i| shares[i].clone()).collect()
}

#[test]
fn round_trips_for_various_thresholds() {
    let s = secret();
    for (k, n) in [(2u8, 3u8), (3, 5), (5, 5), (2, 2), (10, 16)] {
        let shares = split(&s, k, n).unwrap();
        assert_eq!(shares.len(), n as usize);
        let got = combine(&shares[..k as usize]).unwrap();
        assert_eq!(got, s, "k={k} n={n}");
    }
}

#[test]
fn any_threshold_subset_recovers() {
    let s = secret();
    let (k, n) = (3usize, 5usize);
    let shares = split(&s, k as u8, n as u8).unwrap();
    for combo in combos(n, k) {
        assert_eq!(combine(&pick(&shares, &combo)).unwrap(), s, "{combo:?}");
    }
}

#[test]
fn more_than_threshold_also_recovers() {
    let s = secret();
    let shares = split(&s, 3, 5).unwrap();
    assert_eq!(combine(&shares[..4]).unwrap(), s);
    assert_eq!(combine(&shares).unwrap(), s);
}

#[test]
fn fewer_than_threshold_is_refused() {
    let s = secret();
    let shares = split(&s, 3, 5).unwrap();
    match combine(&shares[..2]) {
        Err(Error::Insufficient { have: 2, need: 3 }) => {}
        other => panic!("expected Insufficient, got {other:?}"),
    }
}

#[test]
fn a_corrupted_share_is_caught() {
    let s = secret();
    let shares = split(&s, 3, 4).unwrap();

    // Flip a byte of the first share's body through its portable encoding.
    let head = 1 + 4 + 1 + 1 + 16 + 1;
    let mut bytes = shares[0].encode();
    bytes[head] ^= 0xff;
    let corrupted = Share::decode(&bytes).unwrap();

    let set = vec![corrupted, shares[1].clone(), shares[2].clone()];
    assert!(matches!(combine(&set), Err(Error::Integrity)));
}

#[test]
fn shares_from_different_splits_do_not_mix() {
    let s = secret();
    let a = split(&s, 2, 3).unwrap();
    let b = split(&s, 2, 3).unwrap(); // same secret, fresh id and polynomials
    let mixed = vec![a[0].clone(), b[1].clone()];
    assert!(matches!(combine(&mixed), Err(Error::MixedSet)));
}

#[test]
fn duplicate_index_is_rejected() {
    let s = secret();
    let shares = split(&s, 2, 3).unwrap();
    let dup = vec![shares[0].clone(), shares[0].clone()];
    match combine(&dup) {
        Err(Error::Duplicate(x)) => assert_eq!(x, shares[0].index()),
        other => panic!("expected Duplicate, got {other:?}"),
    }
}

#[test]
fn hex_round_trip_then_recover() {
    let s = secret();
    let shares = split(&s, 3, 5).unwrap();
    let texts: Vec<String> = shares.iter().map(|sh| sh.to_hex()).collect();

    for (sh, t) in shares.iter().zip(&texts) {
        assert_eq!(Share::from_hex(t).unwrap(), *sh);
    }
    let revived: Vec<Share> = texts[..3]
        .iter()
        .map(|t| Share::from_hex(t).unwrap())
        .collect();
    assert_eq!(combine(&revived).unwrap(), s);
}

#[test]
fn rejects_bad_parameters() {
    assert!(matches!(split(&[], 2, 3), Err(Error::Params(_))));
    assert!(matches!(split(&secret(), 0, 3), Err(Error::Params(_))));
    assert!(matches!(split(&secret(), 4, 3), Err(Error::Params(_))));
}

#[test]
fn threshold_one_is_redundant_copies() {
    let s = secret();
    let shares = split(&s, 1, 3).unwrap();
    for sh in &shares {
        assert_eq!(combine(std::slice::from_ref(sh)).unwrap(), s);
    }
}

#[test]
fn a_lone_share_cannot_reconstruct_under_a_real_threshold() {
    let s = secret();
    let shares = split(&s, 2, 3).unwrap();
    for sh in &shares {
        assert!(combine(std::slice::from_ref(sh)).is_err());
    }
}
