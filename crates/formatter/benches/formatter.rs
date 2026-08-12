use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use worsier_formatter::{
    FormatConfig, benchmark_parse, benchmark_rewrite, benchmark_verify, resolve_config,
};

fn formatter_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("formatter");
    group.sample_size(10);
    let no_verify = resolve_config(FormatConfig {
        verify_ast: false,
        ..FormatConfig::default()
    })
    .unwrap();

    for (name, bytes) in [("small", 512), ("50kb", 50 * 1024), ("1mb", 1024 * 1024)] {
        let source = source_with_size(bytes);
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("parse", name),
            &source,
            |bencher, source| {
                bencher.iter(|| {
                    benchmark_parse(Path::new("benchmark.ts"), black_box(source)).unwrap();
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("import_rewrite", name),
            &source,
            |bencher, source| {
                bencher.iter(|| {
                    benchmark_rewrite(Path::new("benchmark.ts"), black_box(source), &no_verify)
                        .unwrap();
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("verify", name),
            &source,
            |bencher, source| {
                bencher.iter(|| {
                    benchmark_verify(Path::new("benchmark.ts"), black_box(source)).unwrap();
                });
            },
        );
    }
    group.finish();
}

fn source_with_size(minimum_bytes: usize) -> String {
    let mut source = String::with_capacity(minimum_bytes + 128);
    let mut index = 0;
    while source.len() < minimum_bytes {
        writeln!(
            source,
            "import {{ value{index}, type Type{index} }} from 'package-{index}';"
        )
        .unwrap();
        writeln!(
            source,
            "const value{index}={{ index:{index},items:[1,2,3],active:true }};"
        )
        .unwrap();
        index += 1;
    }
    source
}

criterion_group!(benches, formatter_benchmarks);
criterion_main!(benches);
