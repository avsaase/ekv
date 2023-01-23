to run fuzzing:

    cargo run --release --example smoke
    rm fuzz/corpus/read/*
    mv out.bin fuzz/corpus/read
    cargo fuzz run read --sanitizer none -j10 -- -timeout=1 -max_len=32768