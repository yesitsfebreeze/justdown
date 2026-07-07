go := "/tmp/go/bin/go"
bin := "bin/jd"

default: run

build:
    {{go}} build -o {{bin}} ./src/cmd/jd

run *args: build
    ./{{bin}} {{args}}
