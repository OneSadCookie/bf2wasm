const fs = require("fs");
async function main() {
    const env = {
        putc: val => process.stdout.write(String.fromCharCode(val)),
        getc: () => process.stdin.read(1).charCodeAt(0),
        memory: new WebAssembly.Memory({
            initial: 1,
            maximum: 1
        })
    };
    const bytes = fs.readFileSync("target/bf.wasm");
    const module = await WebAssembly
        .instantiate(bytes, { env })
        .then(res => res.instance.exports);
    const result = module.main();
    console.log("main:", result);
}
main();
