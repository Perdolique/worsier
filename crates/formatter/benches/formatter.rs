use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use worsier_formatter::{
    FormatConfig, benchmark_index, benchmark_parse, format_text, prepare_document, resolve_config,
};

fn formatter_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("formatter");
    group.sample_size(10);
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
            BenchmarkId::new("node_comment_index", name),
            &source,
            |bencher, source| {
                bencher.iter(|| {
                    benchmark_index(Path::new("benchmark.ts"), black_box(source)).unwrap()
                });
            },
        );

        let no_verify = resolve_config(FormatConfig {
            verify_ast: false,
            ..FormatConfig::default()
        })
        .unwrap();
        group.bench_with_input(
            BenchmarkId::new("ir_generation", name),
            &source,
            |bencher, source| {
                bencher.iter(|| {
                    prepare_document(Path::new("benchmark.ts"), black_box(source), &no_verify)
                        .unwrap()
                });
            },
        );
        let prepared = prepare_document(Path::new("benchmark.ts"), &source, &no_verify).unwrap();
        group.bench_function(BenchmarkId::new("print", name), |bencher| {
            bencher.iter(|| black_box(prepared.render(&no_verify)));
        });
        group.bench_with_input(
            BenchmarkId::new("verify_false", name),
            &source,
            |bencher, source| {
                bencher.iter(|| {
                    format_text(Path::new("benchmark.ts"), black_box(source), &no_verify).unwrap()
                });
            },
        );
        let verify = resolve_config(FormatConfig::default()).unwrap();
        group.bench_with_input(
            BenchmarkId::new("verify_true", name),
            &source,
            |bencher, source| {
                bencher.iter(|| {
                    format_text(Path::new("benchmark.ts"), black_box(source), &verify).unwrap()
                });
            },
        );
    }
    group.finish();

    let project = [
        source_with_size(8 * 1024),
        "export interface User<T> { id: number; value: T; }\n".repeat(128),
        "export const View = ({ title }: { title: string }) => <h1>{title}</h1>;\n".repeat(64),
    ];
    let verify = resolve_config(FormatConfig::default()).unwrap();
    criterion.bench_function("mixed_project", |bencher| {
        bencher.iter(|| {
            for (index, source) in project.iter().enumerate() {
                let extension = if index == 2 { "tsx" } else { "ts" };
                let file_name = format!("project-{index}.{extension}");
                black_box(format_text(Path::new(&file_name), source, &verify).unwrap());
            }
        });
    });
}

fn source_with_size(minimum_bytes: usize) -> String {
    let mut source = String::with_capacity(minimum_bytes + 128);
    let mut index = 0;
    while source.len() < minimum_bytes {
        writeln!(
            source,
            "const value{index} = {{ index: {index}, items: [1, 2, 3], active: true }};"
        )
        .unwrap();
        index += 1;
    }
    source
}

criterion_group!(benches, formatter_benchmarks);
criterion_main!(benches);
