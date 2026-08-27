fn main() {
    for p in std::env::args().skip(1) {
        let path = std::path::Path::new(&p);
        let risk = vardo_sync::storage_risk::assess(path);
        println!("{p}\n  risky: {}\n  {}\n", risk.is_risky(), risk.explain());
    }
}
