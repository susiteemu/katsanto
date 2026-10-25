//! Time the embedded solver: `--example solve -- PLAYER_JS N_CHALLENGE`
use katsanto_yt::solver::{Solver, SolverScripts};
use std::time::Instant;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let player = std::fs::read_to_string(args.next().unwrap()).unwrap();
    let n = vec![args.next().unwrap()];
    let t = Instant::now();
    let solver = Solver::spawn(SolverScripts::vendored()).unwrap();
    println!("init: {:?}", t.elapsed());
    let t = Instant::now();
    let (sol, pre) = solver.solve_player(&player, &n, &[]).await.unwrap();
    println!(
        "preprocess+solve: {:?}, preprocessed {} bytes, n -> {:?}",
        t.elapsed(),
        pre.len(),
        sol.n
    );
    let t = Instant::now();
    let sol = solver.solve(&pre, &n, &[]).await.unwrap();
    println!(
        "solve from preprocessed: {:?}, n -> {:?}",
        t.elapsed(),
        sol.n
    );
}
