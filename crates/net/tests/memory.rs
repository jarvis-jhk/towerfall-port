//! Full lobby + GGRS matches between several sessions in one process, over
//! in-memory rooms (no Warpzone server needed).

use std::time::{Duration, Instant};
use tfp_net::toy::ToyMachine;
use tfp_net::{
    DesyncPolicy, LinkSim, MachineId, MatchSettings, MemHub, NetConfig, NetEvent, NetSession,
    SlotRequest, TeamMode,
};

const STEP: Duration = Duration::from_micros(16_667);

fn slots(m: usize, n: usize) -> Vec<SlotRequest> {
    (0..n)
        .map(|i| SlotRequest {
            name: format!("p{m}.{i}"),
            archer: (m * 3 + i) as u8,
            skin: 0,
            team: Some((i % 2) as u8),
        })
        .collect()
}

#[derive(Default)]
struct Case {
    locals: Vec<usize>,
    spectators: usize,
    link: LinkSim,
    target: i32,
    /// (machine, frame, crash) — crash = just stop polling (no goodbye).
    leave: Vec<(usize, i32, bool)>,
    corrupt: Option<(usize, i32)>,
    policy: DesyncPolicy,
}

struct Run {
    ms: Vec<ToyMachine>,
    left: Vec<bool>,
    desync_reports: usize,
}

impl Run {
    fn rollbacks(&self) -> u64 {
        self.ms
            .iter()
            .filter_map(|m| m.net.last_match_stats())
            .map(|s| s.rollbacks)
            .sum()
    }

    /// Checksums of the target frame of every machine still there; asserts
    /// every one got an agreeing `MatchEnded`.
    fn agreed_checksums(&self) -> Vec<u128> {
        let mut out = Vec::new();
        for (i, m) in self.ms.iter().enumerate() {
            if self.left[i] {
                continue;
            }
            let r = m
                .result
                .clone()
                .unwrap_or_else(|| panic!("m{i}: no result"));
            assert!(r.agreed, "m{i}: reports disagree: {:?}", r.reports);
            assert!(r.desyncs.is_empty(), "m{i}: {:?}", r.desyncs);
            assert_eq!(m.desyncs_seen, 0);
            out.push(m.target_checksum().expect("target simulated"));
        }
        assert!(out.windows(2).all(|w| w[0] == w[1]), "{out:x?}");
        out
    }
}

fn run(c: Case) -> Run {
    let hub = MemHub::new();
    let n = c.locals.len() + c.spectators;
    let mut ms: Vec<ToyMachine> = (0..n)
        .map(|i| {
            let mut cfg = NetConfig::new("test");
            cfg.link_sim = c.link;
            cfg.desync_policy = c.policy;
            let mut net = NetSession::memory(hub.join(&format!("m{i}")), cfg);
            match c.locals.get(i) {
                Some(&k) => net.set_local_slots(slots(i, k)),
                None => net.set_spectate(true),
            }
            let mut m = ToyMachine::new(net, c.target, i as u64 + 7);
            if c.corrupt.is_some_and(|(who, _)| who == i) {
                m.corrupt_at = c.corrupt.map(|x| x.1);
            }
            m
        })
        .collect();
    ms[0]
        .net
        .set_settings(MatchSettings {
            team_mode: TeamMode::Teams(2),
            archers: 8,
            mode: "Team Deathmatch".into(),
            map: "Sacred Ground".into(),
            game: vec![1, 2, 3],
        })
        .unwrap();
    let t0 = Instant::now();
    let mut started = false;
    let mut next = Instant::now();
    let mut left = vec![false; n];
    let mut desync_reports = 0;
    loop {
        assert!(t0.elapsed() < Duration::from_secs(120), "timeout");
        for (i, m) in ms.iter_mut().enumerate() {
            if left[i] {
                continue;
            }
            for e in m.poll() {
                if let NetEvent::DesyncReported(_) = e {
                    desync_reports += 1;
                }
            }
        }
        if !started
            && ms[0].net.lobby().machines.len() == n
            && ms[1..].iter().all(|m| m.net.me().is_some())
        {
            let start = ms[0].net.start_match(Some(1234)).unwrap();
            assert_eq!(start.players.len(), c.locals.iter().sum::<usize>().min(12));
            assert_eq!(start.spectators.len(), c.spectators);
            started = true;
        }
        if Instant::now() >= next {
            next += STEP;
            for (i, m) in ms.iter_mut().enumerate() {
                if left[i] {
                    continue;
                }
                m.step();
                for &(who, at, crash) in &c.leave {
                    if who == i && !left[i] && m.net.current_frame() >= at {
                        if !crash {
                            m.net.leave();
                        }
                        left[i] = true;
                    }
                }
            }
        }
        if started
            && ms
                .iter()
                .enumerate()
                .all(|(i, m)| left[i] || m.result.is_some() || m.aborted.is_some())
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Run {
        ms,
        left,
        desync_reports,
    }
}

#[test]
fn three_machines_mixed_locals() {
    let r = run(Case {
        locals: vec![2, 1, 3],
        target: 300,
        ..Default::default()
    });
    assert_eq!(r.agreed_checksums().len(), 3);
}

#[test]
fn four_by_three_rollback_under_latency_and_loss() {
    let r = run(Case {
        locals: vec![3, 3, 3, 3],
        link: LinkSim {
            delay_ms: 40,
            jitter_ms: 15,
            loss: 0.05,
        },
        target: 400,
        ..Default::default()
    });
    assert_eq!(r.agreed_checksums().len(), 4);
    assert!(r.rollbacks() > 0, "latency should cause rollbacks");
}

#[test]
fn twelve_machines_one_slot() {
    let r = run(Case {
        locals: vec![1; 12],
        link: LinkSim {
            delay_ms: 15,
            jitter_ms: 5,
            loss: 0.02,
        },
        target: 300,
        ..Default::default()
    });
    assert_eq!(r.agreed_checksums().len(), 12);
}

#[test]
fn twelve_local_one_machine() {
    let r = run(Case {
        locals: vec![12],
        target: 200,
        ..Default::default()
    });
    assert_eq!(r.agreed_checksums().len(), 1);
}

#[test]
fn machine_leaves_mid_match() {
    let r = run(Case {
        locals: vec![1, 2, 1],
        link: LinkSim {
            delay_ms: 20,
            jitter_ms: 5,
            loss: 0.0,
        },
        target: 500,
        leave: vec![(2, 150, false)],
        ..Default::default()
    });
    assert_eq!(r.agreed_checksums().len(), 2);
    // Its archer was inert, then removed at the toy's round end (frame 600 >
    // target: still there but marked gone).
    let st = &r.ms[0].runner.as_ref().unwrap().state;
    assert!(st.dots[3].gone && !st.dots[0].gone);
}

#[test]
fn machines_crash_mid_match() {
    let r = run(Case {
        locals: vec![2, 1, 3, 1, 2],
        link: LinkSim {
            delay_ms: 30,
            jitter_ms: 10,
            loss: 0.03,
        },
        target: 700,
        leave: vec![(1, 150, true), (3, 300, true), (4, 310, false)],
        ..Default::default()
    });
    assert_eq!(r.agreed_checksums().len(), 2);
    assert!(r.ms[0].net.last_match_stats().unwrap().resumes >= 2);
}

#[test]
fn spectators_follow_the_match() {
    let r = run(Case {
        locals: vec![2, 2],
        spectators: 2,
        link: LinkSim {
            delay_ms: 25,
            jitter_ms: 10,
            loss: 0.03,
        },
        target: 400,
        ..Default::default()
    });
    // Players and spectators all simulated frame 400 identically.
    assert_eq!(r.agreed_checksums().len(), 4);
    assert!(r.ms[2].net.last_match_stats().is_some());
}

#[test]
fn spectator_survives_a_drop() {
    let r = run(Case {
        locals: vec![1, 1, 1],
        spectators: 1,
        link: LinkSim {
            delay_ms: 20,
            jitter_ms: 5,
            loss: 0.02,
        },
        target: 500,
        leave: vec![(2, 200, true)],
        ..Default::default()
    });
    assert_eq!(r.agreed_checksums().len(), 3);
}

#[test]
fn desync_is_detected_and_reported() {
    let r = run(Case {
        locals: vec![1, 1, 1],
        target: 300,
        corrupt: Some((1, 100)),
        ..Default::default()
    });
    // Machine 1 and the others see the desync; the host collects reports.
    assert!(r.ms[1].desyncs_seen > 0, "corrupted machine notices");
    assert!(r.ms[0].desyncs_seen > 0 && r.ms[2].desyncs_seen > 0);
    assert!(r.desync_reports > 0);
    for m in &r.ms {
        let res = m.result.as_ref().expect("match still ends");
        assert!(!res.agreed, "final checksums differ");
        assert!(!res.desyncs.is_empty());
        assert!(
            res.desyncs
                .iter()
                .all(|d| d.frame >= 100 && d.local != d.remote)
        );
    }
}

#[test]
fn desync_abort_policy() {
    let r = run(Case {
        locals: vec![1, 2],
        target: 600,
        corrupt: Some((0, 50)),
        policy: DesyncPolicy::Abort,
        ..Default::default()
    });
    for m in &r.ms {
        assert!(m.result.is_none());
        let why = m.aborted.as_deref().unwrap_or_default();
        assert!(why.contains("desync"), "{why}");
    }
}

#[test]
fn rejoin_is_recognised_and_plays_next_match() {
    let hub = MemHub::new();
    let mk = |name: &str, account: i64| {
        let mut net = NetSession::memory(hub.join_as(name, account), NetConfig::new("test"));
        net.set_local_slots(slots(account as usize, 1));
        ToyMachine::new(net, 120, account as u64)
    };
    let mut host = mk("host", 100);
    let mut guest = mk("guest", 200);
    let pump = |ms: &mut [&mut ToyMachine]| {
        for _ in 0..20 {
            for m in ms.iter_mut() {
                m.poll();
            }
        }
    };
    pump(&mut [&mut host, &mut guest]);
    assert_eq!(guest.net.me(), Some(MachineId(1)));
    guest.net.leave();
    drop(guest);
    pump(&mut [&mut host]);
    let mut again = mk("guest", 200);
    pump(&mut [&mut host, &mut again]);
    let (machine, from) = host
        .net
        .lobby()
        .machines
        .iter()
        .find(|m| m.account == 200)
        .map(|m| (m.machine, m.rejoined_from))
        .expect("rejoined");
    assert_eq!(machine, MachineId(2), "machine ids are never reused");
    assert_eq!(from, Some(MachineId(1)));
    assert_eq!(again.net.me(), Some(MachineId(2)));
    let ms = host.net.start_match(Some(9)).unwrap();
    assert_eq!(ms.machines(), vec![MachineId(0), MachineId(2)]);
}

#[test]
fn machine_info_reaches_the_roster() {
    let hub = MemHub::new();
    let mut a: NetSession<tfp_net::toy::Toy> =
        NetSession::memory(hub.join("a"), NetConfig::new("t"));
    let mut b: NetSession<tfp_net::toy::Toy> =
        NetSession::memory(hub.join("b"), NetConfig::new("t"));
    a.set_machine_info(vec![1]);
    b.set_machine_info(vec![2, 2]);
    let pump = |a: &mut NetSession<_>, b: &mut NetSession<_>| {
        for _ in 0..10 {
            a.poll();
            b.poll();
            while a.next_event().is_some() {}
            while b.next_event().is_some() {}
        }
    };
    pump(&mut a, &mut b);
    let infos = |n: &NetSession<tfp_net::toy::Toy>| -> Vec<Vec<u8>> {
        n.lobby().machines.iter().map(|m| m.info.clone()).collect()
    };
    assert_eq!(
        infos(&a),
        vec![vec![1], vec![2, 2]],
        "host sees both (Hello)"
    );
    assert_eq!(infos(&b), infos(&a), "guests get the host's roster");
    b.set_machine_info(vec![3]);
    pump(&mut a, &mut b);
    assert_eq!(infos(&b)[1], vec![3], "updates travel");
}
