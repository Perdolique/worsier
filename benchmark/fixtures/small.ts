import{type BenchmarkConfig,runBenchmark}from'benchmark-package';const config:BenchmarkConfig={files:['one.ts','two.ts'],iterations:10,enabled:true};runBenchmark(config);
