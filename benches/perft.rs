use criterion::{criterion_group, criterion_main, Criterion};
use shogi::bitboard::Factory;
use shogi::Position;

fn bench_perft(c: &mut Criterion) {
    Factory::init();

    let mut pos = Position::new();
    pos.set_sfen("l6nl/5+P1gk/2np1S3/p1p4Pp/3P2Sp1/1PPb2P1P/P5GS1/R8/LN4bKL w RGgsn5p 1")
        .expect("failed to parse SFEN string");
    let depth = 2;
    let expected_count = 28684;

    c.bench_function("perft", |b| {
        b.iter(|| test_perft(&pos, depth, expected_count))
    });
}

fn test_perft(pos: &Position, depth: usize, expected_count: usize) {
    let mut pos = pos.clone();
    let cnt = perft(&mut pos, depth);

    assert_eq!(expected_count, cnt);
}

fn perft_leaf(pos: &mut Position) -> usize {
    let mut cnt = 0;

    for m in pos.all_move_candidates(pos.side_to_move()) {
        if pos.is_legal(m) {
            cnt += 1;
        }
    }

    cnt
}

fn perft(pos: &mut Position, depth: usize) -> usize {
    if depth == 0 {
        return 1;
    } else if depth == 1 {
        return perft_leaf(pos);
    }
    let mut cnt = 0;

    for m in pos.all_move_candidates(pos.side_to_move()) {
        match pos.make_move(m) {
            Ok(_) => {
                cnt += perft(pos, depth - 1);
                let _ = pos.unmake_move();
            }
            Err(_) => {}
        }
    }

    cnt
}

criterion_group!(benches, bench_perft);
criterion_main!(benches);
