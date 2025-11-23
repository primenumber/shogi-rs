#![cfg(test)]
use super::*;
use crate::position::BBFactory;
use crate::square::consts::*;

fn setup() {
    BBFactory::init();
}

#[test]
fn new() {
    setup();

    let pos = Position::new();

    for i in 0..9 {
        for j in 0..9 {
            let sq = Square::new(i, j).unwrap();
            assert_eq!(None, *pos.piece_at(sq));
        }
    }
}

#[test]
fn in_check() {
    setup();

    let test_cases = [
        (
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            false,
            false,
        ),
        ("9/3r5/9/9/6B2/9/9/9/3K5 b P 1", true, false),
        (
            "ln2r1knl/2gb1+Rg2/4Pp1p1/p1pp1sp1p/1N2pN1P1/2P2PP2/PP1G1S2R/1SG6/LK6L w 2PSp 1",
            false,
            true,
        ),
        (
            "lnsg1gsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSG1GSNL b - 1",
            false,
            false,
        ),
    ];

    let mut pos = Position::new();
    for case in test_cases.iter() {
        pos.set_sfen(case.0).expect("failed to parse SFEN string");
        assert_eq!(case.1, pos.in_check(Color::Black));
        assert_eq!(case.2, pos.in_check(Color::White));
    }
}

#[test]
fn find_king() {
    setup();

    let test_cases = [
        (
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            Some(SQ_5I),
            Some(SQ_5A),
        ),
        ("9/3r5/9/9/6B2/9/9/9/3K5 b P 1", Some(SQ_6I), None),
        (
            "ln2r1knl/2gb1+Rg2/4Pp1p1/p1pp1sp1p/1N2pN1P1/2P2PP2/PP1G1S2R/1SG6/LK6L w 2PSp 1",
            Some(SQ_8I),
            Some(SQ_3A),
        ),
        (
            "lnsg1gsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSG1GSNL b - 1",
            None,
            None,
        ),
    ];

    let mut pos = Position::new();
    for case in test_cases.iter() {
        pos.set_sfen(case.0).expect("failed to parse SFEN string");
        assert_eq!(case.1, pos.find_king(Color::Black));
        assert_eq!(case.2, pos.find_king(Color::White));
    }
}

#[test]
fn player_bb() {
    setup();

    let cases: &[(&str, &[Square], &[Square])] = &[
        (
            "R6gk/9/8p/9/4p4/9/9/8L/B8 b - 1",
            &[SQ_9A, SQ_1H, SQ_9I],
            &[SQ_2A, SQ_1A, SQ_1C, SQ_5E],
        ),
        ("9/3r5/9/9/6B2/9/9/9/3K5 b P 1", &[SQ_3E, SQ_6I], &[SQ_6B]),
    ];

    let mut pos = Position::new();
    for case in cases {
        pos.set_sfen(case.0).expect("faled to parse SFEN string");
        let black = pos.player_bb(Color::Black);
        let white = pos.player_bb(Color::White);

        assert_eq!(case.1.len(), black.count() as usize);
        for sq in case.1 {
            assert!((black & *sq).is_any());
        }

        assert_eq!(case.2.len(), white.count() as usize);
        for sq in case.2 {
            assert!((white & *sq).is_any());
        }
    }
}

#[test]
fn pinned_bb() {
    setup();

    let cases: &[(&str, &[Square], &[Square])] = &[("R6gk/9/8p/9/4p4/9/9/8L/B8 b - 1", &[], &[SQ_2A, SQ_1C, SQ_5E])];

    let mut pos = Position::new();
    for case in cases {
        pos.set_sfen(case.0).expect("faled to parse SFEN string");
        let black = pos.pinned_bb(Color::Black);
        let white = pos.pinned_bb(Color::White);

        assert_eq!(case.1.len(), black.count());
        for sq in case.1 {
            assert!((&black & *sq).is_any());
        }

        assert_eq!(case.2.len(), white.count());
        for sq in case.2 {
            assert!((&white & *sq).is_any());
        }
    }
}

#[test]
fn move_candidates() {
    setup();

    let mut pos = Position::new();
    pos.set_sfen("lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1")
        .expect("failed to parse SFEN string");

    let mut sum = 0;
    for sq in Square::iter() {
        let pc = pos.piece_at(sq);

        if let Some(pc) = *pc {
            if pc.color == pos.side_to_move() {
                sum += pos.move_candidates(sq, pc).count();
            }
        }
    }

    assert_eq!(30, sum);
}

#[test]
fn make_normal_move() {
    setup();

    let base_sfen = "l6nl/5+P1gk/2np1S3/p1p4Pp/3P2Sp1/1PPb2P1P/P5GS1/R8/LN4bKL w GR5pnsg 1";
    let test_cases = [
        (SQ_2B, SQ_2C, false, true),
        (SQ_7C, SQ_6E, false, true),
        (SQ_3I, SQ_4H, true, true),
        (SQ_6F, SQ_9I, true, true),
        (SQ_2B, SQ_2C, false, true),
        (SQ_9C, SQ_9D, false, false),
        (SQ_9B, SQ_8B, false, false),
        (SQ_9B, SQ_9D, false, false),
        (SQ_2B, SQ_2C, true, false),
    ];

    let mut pos = Position::new();
    for case in test_cases.iter() {
        pos.set_sfen(base_sfen).expect("failed to parse SFEN string");
        assert_eq!(case.3, pos.make_normal_move(case.0, case.1, case.2).is_ok());
    }

    // Leaving the checked king is illegal.
    pos.set_sfen("9/3r5/9/9/6B2/9/9/9/3K5 b P 1")
        .expect("failed to parse SFEN string");
    assert!(pos.make_normal_move(SQ_6I, SQ_6H, false).is_err());
    pos.set_sfen("9/3r5/9/9/6B2/9/9/9/3K5 b P 1")
        .expect("failed to parse SFEN string");
    assert!(pos.make_normal_move(SQ_6I, SQ_7I, false).is_ok());
}

#[test]
fn make_drop_move() {
    setup();

    let base_sfen = "l6nl/5+P1gk/2np1S3/p1p4Pp/3P2Sp1/1PPb2P1P/P5GS1/R8/LN4bKL w GR5pnsg 1";
    let test_cases = [
        (SQ_5E, PieceType::Pawn, true),
        (SQ_5E, PieceType::Rook, false),
        (SQ_9A, PieceType::Pawn, false),
        (SQ_6F, PieceType::Pawn, false),
        (SQ_9B, PieceType::Pawn, false),
        (SQ_5I, PieceType::Pawn, false),
    ];

    let mut pos = Position::new();
    for case in test_cases.iter() {
        pos.set_sfen(base_sfen).expect("failed to parse SFEN string");
        assert_eq!(
            case.2,
            pos.make_move(Move::Drop {
                to: case.0,
                piece_type: case.1,
            })
            .is_ok()
        );
    }
}

#[test]
fn nifu() {
    setup();

    let ng_cases = [(
        "ln1g5/1ks1g3l/1p2p1n2/p1pGs2rp/1P1N1ppp1/P1SB1P2P/1S1p1bPP1/LKG6/4R2NL \
         w 2Pp 91",
        SQ_6C,
    )];
    let ok_cases = [(
        "ln1g5/1ks1g3l/1p2p1n2/p1pGs2rp/1P1N1ppp1/P1SB1P2P/1S1+p1bPP1/LKG6/4R2NL \
         w 2Pp 91",
        SQ_6C,
    )];

    let mut pos = Position::new();
    for (i, case) in ng_cases.iter().enumerate() {
        pos.set_sfen(case.0).expect("failed to parse SFEN string");
        assert_eq!(
            Some(MoveError::Nifu),
            pos.make_move(Move::Drop {
                to: case.1,
                piece_type: PieceType::Pawn,
            })
            .err(),
            "failed at #{i}"
        );
    }

    for (i, case) in ok_cases.iter().enumerate() {
        pos.set_sfen(case.0).expect("failed to parse SFEN string");
        assert!(
            pos.make_move(Move::Drop {
                to: case.1,
                piece_type: PieceType::Pawn,
            })
            .is_ok(),
            "failed at #{i}"
        );
    }
}

#[test]
fn uchifuzume() {
    setup();

    let ng_cases = [
        ("9/9/7sp/6ppk/9/7G1/9/9/9 b P 1", SQ_1E),
        ("7nk/9/7S1/6b2/9/9/9/9/9 b P 1", SQ_1B),
        ("7nk/7g1/6BS1/9/9/9/9/9/9 b P 1", SQ_1B),
        ("R6gk/9/7S1/9/9/9/9/9/9 b P 1", SQ_1B),
    ];
    let ok_cases = [
        ("9/9/7pp/6psk/9/7G1/7N1/9/9 b P 1", SQ_1E),
        ("7nk/9/7Sg/6b2/9/9/9/9/9 b P 1", SQ_1B),
        (
            "9/8p/3pG1gp1/2p2kl1N/3P1p1s1/lPP6/2SGBP3/PK1S2+p2/LN7 w RSL3Prbg2n4p 1",
            SQ_8G,
        ),
        ("7k1/5G2l/6B2/9/9/9/9/9/9 b NP 1", SQ_2B),
    ];

    let mut pos = Position::new();
    for (i, case) in ng_cases.iter().enumerate() {
        pos.set_sfen(case.0).expect("failed to parse SFEN string");
        assert_eq!(
            Some(MoveError::Uchifuzume),
            pos.make_move(Move::Drop {
                to: case.1,
                piece_type: PieceType::Pawn,
            })
            .err(),
            "failed at #{i}"
        );
    }

    for (i, case) in ok_cases.iter().enumerate() {
        pos.set_sfen(case.0).expect("failed to parse SFEN string");
        assert!(
            pos.make_move(Move::Drop {
                to: case.1,
                piece_type: PieceType::Pawn,
            })
            .is_ok(),
            "failed at #{i}"
        );
    }
}

#[test]
fn repetition() {
    setup();

    let mut pos = Position::new();
    pos.set_sfen("ln7/ks+R6/pp7/9/9/9/9/9/9 b Ss 1")
        .expect("failed to parse SFEN string");

    for _ in 0..2 {
        assert!(pos.make_drop_move(SQ_7A, PieceType::Silver).is_ok());
        assert!(pos.make_drop_move(SQ_7C, PieceType::Silver).is_ok());
        assert!(pos.make_normal_move(SQ_7A, SQ_8B, true).is_ok());
        assert!(pos.make_normal_move(SQ_7C, SQ_8B, false).is_ok());
    }

    assert!(pos.make_drop_move(SQ_7A, PieceType::Silver).is_ok());
    assert!(pos.make_drop_move(SQ_7C, PieceType::Silver).is_ok());
    assert!(pos.make_normal_move(SQ_7A, SQ_8B, true).is_ok());
    assert_eq!(
        Some(MoveError::Repetition),
        pos.make_normal_move(SQ_7C, SQ_8B, false).err()
    );
}

#[test]
fn percetual_check() {
    setup();

    // Case 1. Starting from a check move.
    let mut pos = Position::new();
    pos.set_sfen("8l/6+P2/6+Rpk/8p/9/7S1/9/9/9 b - 1")
        .expect("failed to parse SFEN string");

    for _ in 0..2 {
        assert!(pos.make_normal_move(SQ_3C, SQ_2B, false).is_ok());
        assert!(pos.make_normal_move(SQ_1C, SQ_2D, false).is_ok());
        assert!(pos.make_normal_move(SQ_2B, SQ_3C, false).is_ok());
        assert!(pos.make_normal_move(SQ_2D, SQ_1C, false).is_ok());
    }
    assert!(pos.make_normal_move(SQ_3C, SQ_2B, false).is_ok());
    assert!(pos.make_normal_move(SQ_1C, SQ_2D, false).is_ok());
    assert!(pos.make_normal_move(SQ_2B, SQ_3C, false).is_ok());
    assert_eq!(
        Some(MoveError::PerpetualCheckWin),
        pos.make_normal_move(SQ_2D, SQ_1C, false).err()
    );

    // Case 2. Starting from an escape move.
    pos.set_sfen("6p1k/9/8+R/9/9/9/9/9/9 w - 1")
        .expect("failed to parse SFEN string");

    for _ in 0..2 {
        assert!(pos.make_normal_move(SQ_1A, SQ_2A, false).is_ok());
        assert!(pos.make_normal_move(SQ_1C, SQ_2C, false).is_ok());
        assert!(pos.make_normal_move(SQ_2A, SQ_1A, false).is_ok());
        assert!(pos.make_normal_move(SQ_2C, SQ_1C, false).is_ok());
    }
    assert!(pos.make_normal_move(SQ_1A, SQ_2A, false).is_ok());
    assert!(pos.make_normal_move(SQ_1C, SQ_2C, false).is_ok());
    assert!(pos.make_normal_move(SQ_2A, SQ_1A, false).is_ok());
    assert_eq!(
        Some(MoveError::PerpetualCheckLose),
        pos.make_normal_move(SQ_2C, SQ_1C, false).err()
    );
}

#[test]
fn unmake_move() {
    setup();

    let mut pos = Position::new();
    let base_sfen = "l6nl/4+p+P1gk/2n2S3/p1p4Pp/3P2Sp1/1PPb2P1P/4+P1GS1/R8/LN4bKL w RG5gsnp 1";
    pos.set_sfen(base_sfen).expect("failed to parse SFEN string");
    let base_state = format!("{pos}");
    println!("{base_state}");
    let test_cases = [
        Move::Drop {
            to: SQ_5E,
            piece_type: PieceType::Pawn,
        },
        // No capture by unpromoted piece
        Move::Normal {
            from: SQ_6F,
            to: SQ_7G,
            promote: false,
        },
        // No capture by promoting piece
        Move::Normal {
            from: SQ_6F,
            to: SQ_7G,
            promote: true,
        },
        // No capture by promoted piece
        Move::Normal {
            from: SQ_5B,
            to: SQ_5A,
            promote: false,
        },
        // Capture of unpromoted piece by unpromoted piece
        Move::Normal {
            from: SQ_6F,
            to: SQ_9I,
            promote: false,
        },
        // Capture of unpromoted piece by promoting piece
        Move::Normal {
            from: SQ_6F,
            to: SQ_9I,
            promote: true,
        },
        // Capture of unpromoted piece by promoted piece
        Move::Normal {
            from: SQ_5B,
            to: SQ_4C,
            promote: false,
        },
        // Capture of promoted piece by unpromoted piece
        Move::Normal {
            from: SQ_6F,
            to: SQ_5G,
            promote: false,
        },
        // Capture of promoted piece by promoting piece
        Move::Normal {
            from: SQ_6F,
            to: SQ_5G,
            promote: true,
        },
        // Capture of promoted piece by promoted piece
        Move::Normal {
            from: SQ_5B,
            to: SQ_4B,
            promote: false,
        },
    ];

    for case in test_cases.iter() {
        pos.set_sfen(base_sfen).expect("failed to parse SFEN string");
        pos.make_move(*case)
            .unwrap_or_else(|_| panic!("failed to make a move: {case}"));
        pos.unmake_move()
            .unwrap_or_else(|_| panic!("failed to unmake a move: {case}"));
        assert_eq!(
            base_sfen,
            pos.to_sfen(),
            "{}",
            format!("sfen unmatch for {case}").as_str()
        );
        assert_eq!(
            base_state,
            format!("{pos}"),
            "{}",
            format!("state unmatch for {case}").as_str()
        );
    }
}

#[test]
fn try_declare_winning() {
    setup();

    let mut pos = Position::new();

    pos.set_sfen("lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1")
        .expect("failed to parse SFEN string");
    assert!(!pos.try_declare_winning(Color::Black));
    assert!(!pos.try_declare_winning(Color::White));

    pos.set_sfen("1K7/+NG+N+NGG3/P+S+P+P+PS3/9/7s1/9/+b+rppp+p+s1+p/3+p1+bk2/9 b R4L7Pgnp 1")
        .expect("failed to parse SFEN string");
    assert!(pos.try_declare_winning(Color::Black));
    assert!(!pos.try_declare_winning(Color::White));

    pos.set_sfen(
        "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/4+p+p+bs1/+r1s4+lk/1g1g3+r1 w \
         Gns2l11p 1",
    )
    .expect("failed to parse SFEN string");
    assert!(!pos.try_declare_winning(Color::Black));
    assert!(pos.try_declare_winning(Color::White));

    pos.set_sfen(
        "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/4+p+p+bs1/+r1s4+lk/1g1g3+r1 b \
         Gns2l11p 1",
    )
    .expect("failed to parse SFEN string");
    assert!(!pos.try_declare_winning(Color::Black));
    assert!(!pos.try_declare_winning(Color::White));

    pos.set_sfen(
        "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/4+p+p+bs1/+r1s4+l1/1g1g3+r1 b \
         Gns2l11p 1",
    )
    .expect("failed to parse SFEN string");
    assert!(!pos.try_declare_winning(Color::Black));
    assert!(!pos.try_declare_winning(Color::White));

    pos.set_sfen(
        "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/1k1+b4P/4+p+p+bs1/+r1s4+l1/1g1g3+r1 b \
         Gns2l11p 1",
    )
    .expect("failed to parse SFEN string");
    assert!(!pos.try_declare_winning(Color::Black));
    assert!(!pos.try_declare_winning(Color::White));

    pos.set_sfen(
        "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/4+p+p+bs1/+r1s4+lk/1g1g3+rG w \
         ns2l11p 1",
    )
    .expect("failed to parse SFEN string");
    assert!(!pos.try_declare_winning(Color::Black));
    assert!(!pos.try_declare_winning(Color::White));

    pos.set_sfen("1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/5+p+bs1/+r1s4+lk/1g1g3+rG w ns2l12p 1")
        .expect("failed to parse SFEN string");
    assert!(!pos.try_declare_winning(Color::Black));
    assert!(!pos.try_declare_winning(Color::White));
}

#[test]
fn set_sfen_normal() {
    setup();

    let mut pos = Position::new();

    pos.set_sfen("lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1")
        .expect("failed to parse SFEN string");

    let filled_squares = [
        (0, 0, PieceType::Lance, Color::White),
        (1, 0, PieceType::Knight, Color::White),
        (2, 0, PieceType::Silver, Color::White),
        (3, 0, PieceType::Gold, Color::White),
        (4, 0, PieceType::King, Color::White),
        (5, 0, PieceType::Gold, Color::White),
        (6, 0, PieceType::Silver, Color::White),
        (7, 0, PieceType::Knight, Color::White),
        (8, 0, PieceType::Lance, Color::White),
        (7, 1, PieceType::Rook, Color::White),
        (1, 1, PieceType::Bishop, Color::White),
        (0, 2, PieceType::Pawn, Color::White),
        (1, 2, PieceType::Pawn, Color::White),
        (2, 2, PieceType::Pawn, Color::White),
        (3, 2, PieceType::Pawn, Color::White),
        (4, 2, PieceType::Pawn, Color::White),
        (5, 2, PieceType::Pawn, Color::White),
        (6, 2, PieceType::Pawn, Color::White),
        (7, 2, PieceType::Pawn, Color::White),
        (8, 2, PieceType::Pawn, Color::White),
        (0, 6, PieceType::Pawn, Color::Black),
        (1, 6, PieceType::Pawn, Color::Black),
        (2, 6, PieceType::Pawn, Color::Black),
        (3, 6, PieceType::Pawn, Color::Black),
        (4, 6, PieceType::Pawn, Color::Black),
        (5, 6, PieceType::Pawn, Color::Black),
        (6, 6, PieceType::Pawn, Color::Black),
        (7, 6, PieceType::Pawn, Color::Black),
        (8, 6, PieceType::Pawn, Color::Black),
        (7, 7, PieceType::Bishop, Color::Black),
        (1, 7, PieceType::Rook, Color::Black),
        (0, 8, PieceType::Lance, Color::Black),
        (1, 8, PieceType::Knight, Color::Black),
        (2, 8, PieceType::Silver, Color::Black),
        (3, 8, PieceType::Gold, Color::Black),
        (4, 8, PieceType::King, Color::Black),
        (5, 8, PieceType::Gold, Color::Black),
        (6, 8, PieceType::Silver, Color::Black),
        (7, 8, PieceType::Knight, Color::Black),
        (8, 8, PieceType::Lance, Color::Black),
    ];

    let empty_squares = [
        (0, 1, 1),
        (2, 1, 5),
        (8, 1, 1),
        (0, 3, 9),
        (0, 4, 9),
        (0, 5, 9),
        (0, 7, 1),
        (2, 7, 5),
        (8, 7, 1),
    ];

    let hand_pieces = [
        (PieceType::Pawn, 0),
        (PieceType::Lance, 0),
        (PieceType::Knight, 0),
        (PieceType::Silver, 0),
        (PieceType::Gold, 0),
        (PieceType::Rook, 0),
        (PieceType::Bishop, 0),
    ];

    for case in filled_squares.iter() {
        let (file, row, pt, c) = *case;
        assert_eq!(
            Some(Piece {
                piece_type: pt,
                color: c,
            }),
            *pos.piece_at(Square::new(file, row).unwrap())
        );
    }

    for case in empty_squares.iter() {
        let (file, row, len) = *case;
        for i in file..(file + len) {
            assert_eq!(None, *pos.piece_at(Square::new(i, row).unwrap()));
        }
    }

    for case in hand_pieces.iter() {
        let (pt, n) = *case;
        assert_eq!(
            n,
            pos.hand(Piece {
                piece_type: pt,
                color: Color::Black,
            })
        );
        assert_eq!(
            n,
            pos.hand(Piece {
                piece_type: pt,
                color: Color::White,
            })
        );
    }

    assert_eq!(Color::Black, pos.side_to_move());
    assert_eq!(1, pos.ply());
}

#[test]
fn to_sfen() {
    setup();

    let test_cases = [
        "7k1/9/7P1/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1",
        "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
        "lnsgk+Lpnl/1p5+B1/p1+Pps1ppp/9/9/9/P+r1PPpPPP/1R7/LNSGKGSN1 w BGP2p \
         1024",
    ];

    let mut pos = Position::new();
    for case in test_cases.iter() {
        pos.set_sfen(case).expect("failed to parse SFEN string");
        assert_eq!(*case, pos.to_sfen());
    }
}

#[test]
fn set_sfen_custom() {
    setup();

    let mut pos = Position::new();
    pos.set_sfen("lnsgk+Lpnl/1p5+B1/p1+Pps1ppp/9/9/9/P+r1PPpPPP/1R7/LNSGKGSN1 w BGP2p 1024")
        .expect("failed to parse SFEN string");

    let filled_squares = [
        (8, 0, PieceType::Lance, Color::White),
        (7, 0, PieceType::Knight, Color::White),
        (6, 0, PieceType::Silver, Color::White),
        (5, 0, PieceType::Gold, Color::White),
        (4, 0, PieceType::King, Color::White),
        (3, 0, PieceType::ProLance, Color::Black),
        (2, 0, PieceType::Pawn, Color::White),
        (1, 0, PieceType::Knight, Color::White),
        (0, 0, PieceType::Lance, Color::White),
        (7, 1, PieceType::Pawn, Color::White),
        (1, 1, PieceType::ProBishop, Color::Black),
        (8, 2, PieceType::Pawn, Color::White),
        (6, 2, PieceType::ProPawn, Color::Black),
        (5, 2, PieceType::Pawn, Color::White),
        (4, 2, PieceType::Silver, Color::White),
        (2, 2, PieceType::Pawn, Color::White),
        (1, 2, PieceType::Pawn, Color::White),
        (0, 2, PieceType::Pawn, Color::White),
        (8, 6, PieceType::Pawn, Color::Black),
        (7, 6, PieceType::ProRook, Color::White),
        (5, 6, PieceType::Pawn, Color::Black),
        (4, 6, PieceType::Pawn, Color::Black),
        (3, 6, PieceType::Pawn, Color::White),
        (2, 6, PieceType::Pawn, Color::Black),
        (1, 6, PieceType::Pawn, Color::Black),
        (0, 6, PieceType::Pawn, Color::Black),
        (7, 7, PieceType::Rook, Color::Black),
        (8, 8, PieceType::Lance, Color::Black),
        (7, 8, PieceType::Knight, Color::Black),
        (6, 8, PieceType::Silver, Color::Black),
        (5, 8, PieceType::Gold, Color::Black),
        (4, 8, PieceType::King, Color::Black),
        (3, 8, PieceType::Gold, Color::Black),
        (2, 8, PieceType::Silver, Color::Black),
        (1, 8, PieceType::Knight, Color::Black),
    ];

    let empty_squares = [
        (0, 1, 1),
        (2, 1, 5),
        (8, 1, 1),
        (3, 2, 1),
        (7, 2, 1),
        (0, 3, 9),
        (0, 4, 9),
        (0, 5, 9),
        (6, 6, 1),
        (0, 7, 7),
        (8, 7, 1),
        (0, 8, 1),
    ];

    let hand_pieces = [
        (
            Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            },
            1,
        ),
        (
            Piece {
                piece_type: PieceType::Gold,
                color: Color::Black,
            },
            1,
        ),
        (
            Piece {
                piece_type: PieceType::Bishop,
                color: Color::Black,
            },
            1,
        ),
        (
            Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            },
            2,
        ),
    ];

    for case in filled_squares.iter() {
        let (file, row, pt, c) = *case;
        assert_eq!(
            Some(Piece {
                piece_type: pt,
                color: c,
            }),
            *pos.piece_at(Square::new(file, row).unwrap())
        );
    }

    for case in empty_squares.iter() {
        let (file, row, len) = *case;
        for i in file..(file + len) {
            assert_eq!(None, *pos.piece_at(Square::new(i, row).unwrap()));
        }
    }

    for case in hand_pieces.iter() {
        let (p, n) = *case;
        assert_eq!(n, pos.hand(p));
    }

    assert_eq!(Color::White, pos.side_to_move());
    assert_eq!(1024, pos.ply());
}

#[test]
fn packed_position_roundtrip() {
    setup();

    let test_cases = [
        // Standard starting position
        "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
        // Position with pieces in hand and promoted pieces (from set_sfen_custom test)
        "lnsgk+Lpnl/1p5+B1/p1+Pps1ppp/9/9/9/P+r1PPpPPP/1R7/LNSGKGSN1 w BGP2p 1024",
        // Position from make_normal_move test
        "l6nl/5+P1gk/2np1S3/p1p4Pp/3P2Sp1/1PPb2P1P/P5GS1/R8/LN4bKL w GR5pnsg 1",
    ];

    for (i, &sfen) in test_cases.iter().enumerate() {
        let mut original_pos = Position::new();
        original_pos
            .set_sfen(sfen)
            .unwrap_or_else(|_| panic!("failed to parse SFEN at case #{i}"));

        // Pack the position
        let serialized = SerializedPosition::from_position(&original_pos);

        // Unpack the position
        let restored_pos = serialized.to_position(1);

        // Verify board pieces match
        for sq in Square::iter() {
            assert_eq!(
                original_pos.piece_at(sq),
                restored_pos.piece_at(sq),
                "piece mismatch at {sq} in case #{i} (SFEN: {sfen})"
            );
        }

        // Verify hand pieces match
        for pt in PieceType::iter().filter(|pt| pt.is_hand_piece()) {
            for c in [Color::Black, Color::White] {
                let piece = Piece {
                    piece_type: pt,
                    color: c,
                };
                assert_eq!(
                    original_pos.hand(piece),
                    restored_pos.hand(piece),
                    "hand mismatch for {piece:?} in case #{i} (SFEN: {sfen})"
                );
            }
        }

        // Verify side to move matches
        assert_eq!(
            original_pos.side_to_move(),
            restored_pos.side_to_move(),
            "side_to_move mismatch in case #{i} (SFEN: {sfen})"
        );

        // Verify bitboards match
        assert_eq!(
            original_pos.state.occupied_bb, restored_pos.state.occupied_bb,
            "occupied_bb mismatch in case #{i} (SFEN: {sfen})"
        );
        for c in [Color::Black, Color::White] {
            assert_eq!(
                original_pos.player_bb(c),
                restored_pos.player_bb(c),
                "color_bb[{c:?}] mismatch in case #{i} (SFEN: {sfen})"
            );
        }
    }
}
