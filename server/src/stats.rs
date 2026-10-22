//! Statistics across sessions: each pilot's best lap, races and wins, all
//! time and today. Pilots are told apart by their pilot id; seats without a
//! pilot (free text or channel name) by their label.

use std::collections::HashMap;

use serde::Serialize;

use crate::db::SessionRow;
use crate::laps::PilotResult;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PilotStats {
    pub pilot_id: Option<i64>,
    /// As shown in the newest session.
    pub label: String,
    pub best_lap_us: Option<i64>,
    /// The fastest `consecutive` laps in a row (within one session).
    pub best_consecutive_us: Option<i64>,
    pub sessions: u32,
    pub races: u32,
    pub wins: u32,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    /// Races started today.
    pub today_races: u32,
    /// Best lap first.
    pub all: Vec<PilotStats>,
    pub today: Vec<PilotStats>,
}

/// The seats of a race in finishing order: more laps first, then whoever got
/// there earlier. Seats without a lap are left out.
pub fn race_order(results: &[PilotResult]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..results.len()).filter(|&i| !results[i].laps.is_empty()).collect();
    order.sort_by_key(|&i| {
        (
            std::cmp::Reverse(results[i].laps.len()),
            results[i].total_us.unwrap_or(i64::MAX),
        )
    });
    order
}

/// `sessions`: newest first, each with its results (in seat order);
/// `consecutive`: how many laps in a row `best_consecutive_us` covers.
pub fn compute(sessions: &[(SessionRow, Vec<PilotResult>)], today_start_us: i64, consecutive: usize) -> Stats {
    let mut all: HashMap<String, PilotStats> = HashMap::new();
    let mut today: HashMap<String, PilotStats> = HashMap::new();
    let mut today_races = 0;
    for (s, results) in sessions {
        let is_today = s.created_us >= today_start_us;
        if is_today && s.config.race {
            today_races += 1;
        }
        // Only finished races have a winner.
        let winner = (s.config.race && s.ended_us.is_some())
            .then(|| race_order(results).first().copied())
            .flatten();
        for (i, (seat, r)) in s.seats.iter().zip(results).enumerate() {
            let key = match seat.pilot_id {
                Some(id) => format!("id:{id}"),
                None => format!("label:{}", seat.label),
            };
            let mut maps = vec![&mut all];
            if is_today {
                maps.push(&mut today);
            }
            for map in maps {
                // Newest first: the first label seen stays.
                let e = map.entry(key.clone()).or_insert_with(|| PilotStats {
                    pilot_id: seat.pilot_id,
                    label: seat.label.clone(),
                    best_lap_us: None,
                    best_consecutive_us: None,
                    sessions: 0,
                    races: 0,
                    wins: 0,
                });
                if r.laps.is_empty() {
                    continue; // on the list, but didn't fly
                }
                e.sessions += 1;
                if s.config.race {
                    e.races += 1;
                }
                if winner == Some(i) {
                    e.wins += 1;
                }
                if let Some(b) = r.best_lap_us {
                    e.best_lap_us = Some(e.best_lap_us.map_or(b, |x| x.min(b)));
                }
                let n = consecutive.max(1);
                if let Some(b) = r
                    .laps
                    .windows(n)
                    .map(|w| w.iter().map(|l| l.duration_us).sum::<i64>())
                    .min()
                {
                    e.best_consecutive_us = Some(e.best_consecutive_us.map_or(b, |x| x.min(b)));
                }
            }
        }
    }
    let sorted = |m: HashMap<String, PilotStats>| {
        let mut v: Vec<PilotStats> = m.into_values().filter(|p| p.sessions > 0).collect();
        v.sort_by_key(|p| p.best_lap_us.unwrap_or(i64::MAX));
        v
    };
    Stats {
        today_races,
        all: sorted(all),
        today: sorted(today),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Seat, SessionConfig};
    use crate::laps::{EndRule, Lap, StartMode};

    fn session(id: i64, created_us: i64, race: bool, seats: &[(Option<i64>, &str)]) -> SessionRow {
        SessionRow {
            id,
            config: SessionConfig {
                race,
                start_mode: StartMode::Holeshot,
                end_rule: EndRule::Laps(2),
                countdown_s: 0,
                min_lap_s: 3.0,
                consecutive: 2,
            },
            created_us,
            start_us: created_us,
            ended_us: Some(created_us + 1),
            levels_file: None,
            seats: seats
                .iter()
                .enumerate()
                .map(|(i, (pilot_id, label))| Seat {
                    channel: i as u8,
                    channel_name: format!("R{}", i + 1),
                    mhz: 5658,
                    pilot_id: *pilot_id,
                    label: label.to_string(),
                })
                .collect(),
        }
    }

    fn result(channel: u8, laps: &[i64]) -> PilotResult {
        let mut t = 0;
        let laps: Vec<Lap> = laps
            .iter()
            .enumerate()
            .map(|(k, d)| {
                t += d;
                Lap {
                    number: k as u32 + 1,
                    duration_us: *d,
                    end_us: t,
                    pass_id: k as i64,
                }
            })
            .collect();
        PilotResult {
            channel,
            holeshot_us: None,
            best_lap_us: laps.iter().map(|l| l.duration_us).min(),
            total_us: laps.last().map(|l| l.end_us),
            laps,
            finished_us: None,
            best_consecutive: None,
            ignored: vec![],
        }
    }

    #[test]
    fn wins_bests_and_today() {
        let yesterday = session(1, 100, true, &[(Some(7), "NITRO"), (None, "R2")]);
        let race = session(2, 1000, true, &[(Some(7), "NITRO"), (None, "R2")]);
        let training = session(3, 2000, false, &[(Some(7), "NITRO")]);
        let sessions = vec![
            (training, vec![result(0, &[17, 18])]),
            (race, vec![result(0, &[20, 21]), result(1, &[19, 19])]), // R2 is faster: wins
            (yesterday, vec![result(0, &[15, 30]), result(1, &[])]),  // NITRO alone flew: wins
        ];
        let s = compute(&sessions, 500, 2);
        assert_eq!(s.today_races, 1);
        let nitro = s.all.iter().find(|p| p.pilot_id == Some(7)).unwrap();
        assert_eq!(
            (nitro.best_lap_us, nitro.sessions, nitro.races, nitro.wins),
            (Some(15), 3, 2, 1)
        );
        let nitro_today = s.today.iter().find(|p| p.pilot_id == Some(7)).unwrap();
        assert_eq!((nitro_today.best_lap_us, nitro_today.wins), (Some(17), 0));
        let r2 = s.today.iter().find(|p| p.label == "R2").unwrap();
        assert_eq!((r2.best_lap_us, r2.wins), (Some(19), 1));
        assert_eq!(s.all[0].pilot_id, Some(7)); // best lap first
        assert_eq!(nitro.best_consecutive_us, Some(35)); // 17 + 18 (training)
    }
}
