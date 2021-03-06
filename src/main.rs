use clap::{App, Arg};
use failure::{Backtrace, Context, Fail, ResultExt};
use std::fmt::{self, Display};
use std::fs;
use walrus::ir::{BinaryOp, ExprId, ExtendedLoad, LoadKind, MemArg, StoreKind};
use walrus::{
    BlockBuilder, FunctionBuilder, FunctionId, LocalId, MemoryId, Module, ModuleConfig, ValType,
};

#[derive(Copy, Clone, Eq, PartialEq, Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "I/O error")]
    Io,

    #[fail(display = "Invalid input")]
    InvalidInput,

    #[fail(display = "Internal error; unable to generate WebAssembly")]
    Ice,
}

#[derive(Debug)]
pub struct Error {
    inner: Context<ErrorKind>,
}

impl Fail for Error {
    fn cause(&self) -> Option<&Fail> {
        self.inner.cause()
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        self.inner.backtrace()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        *self.inner.get_context()
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error {
            inner: Context::new(kind),
        }
    }
}

impl From<Context<ErrorKind>> for Error {
    fn from(inner: Context<ErrorKind>) -> Error {
        Error { inner: inner }
    }
}

struct BfContext {
    memory: MemoryId,
    putc_func: FunctionId,
    getc_func: FunctionId,

    one_byte: MemArg,
    pointer: LocalId,
    zero: ExprId,
    one: ExprId,
    p: ExprId,
    at_p: ExprId,
}

impl BfContext {
    fn build(
        &self,
        bf: &[u8],
        builder: &mut BlockBuilder,
        consume_all: bool,
    ) -> Result<usize, Error> {
        let mut i = 0;
        while i < bf.len() {
            let byte = bf[i];
            match byte {
                b'>' => {
                    let p = builder.binop(BinaryOp::I32Add, self.p, self.one);
                    let set = builder.local_set(self.pointer, p);
                    builder.expr(set);
                    i += 1;
                }
                b'<' => {
                    let p = builder.binop(BinaryOp::I32Sub, self.p, self.one);
                    let set = builder.local_set(self.pointer, p);
                    builder.expr(set);
                    i += 1;
                }
                b'+' => {
                    let at_p = builder.binop(BinaryOp::I32Add, self.at_p, self.one);
                    let store = builder.store(
                        self.memory,
                        StoreKind::I32_8 { atomic: false },
                        self.one_byte,
                        self.p,
                        at_p,
                    );
                    builder.expr(store);
                    i += 1;
                }
                b'-' => {
                    let at_p = builder.binop(BinaryOp::I32Sub, self.at_p, self.one);
                    let store = builder.store(
                        self.memory,
                        StoreKind::I32_8 { atomic: false },
                        self.one_byte,
                        self.p,
                        at_p,
                    );
                    builder.expr(store);
                    i += 1;
                }
                b'.' => {
                    let call = builder.call(self.putc_func, Box::new([self.at_p]));
                    builder.expr(call);
                    i += 1;
                }
                b',' => {
                    let at_p = builder.call(self.getc_func, Box::new([]));
                    let store = builder.store(
                        self.memory,
                        StoreKind::I32_8 { atomic: false },
                        self.one_byte,
                        self.p,
                        at_p,
                    );
                    builder.expr(store);
                    i += 1;
                }
                b'[' => {
                    let mut loop_wrapper = builder.block(Box::new([]), Box::new([]));
                    let break_label = loop_wrapper.id();
                    let mut loop_body = loop_wrapper.loop_(Box::new([]));
                    let continue_label = loop_body.id();
                    let eq_zero = loop_body.binop(BinaryOp::I32Eq, self.at_p, self.zero);
                    let break_ = loop_body.br_if(eq_zero, break_label, Box::new([]));
                    loop_body.expr(break_);
                    i += 1;
                    i += self.build(&bf[i..], &mut loop_body, false)?;
                    let continue_ = loop_body.br(continue_label, Box::new([]));
                    loop_body.expr(continue_);
                    drop(loop_body);
                    loop_wrapper.expr(From::from(continue_label));
                    drop(loop_wrapper);
                    builder.expr(From::from(break_label));
                }
                b']' => {
                    i += 1;
                    break;
                }
                _ => {
                    Err(ErrorKind::InvalidInput)?;
                }
            }
        }
        if consume_all && i < bf.len() {
            Err(ErrorKind::InvalidInput)?;
        } else if !consume_all && i == bf.len() {
            Err(ErrorKind::InvalidInput)?;
        }

        Ok(i)
    }
}

fn main() -> Result<(), Error> {
    let matches = App::new("bf2wasm")
        .version("0.1")
        .author("Keith Bauer <onesadcookie@gmail.com>")
        .about("Convert Brainfuck to WebAssembly")
        .arg(
            Arg::with_name("input")
                .short("i")
                .long("input")
                .value_name("FILE.bf")
                .help("The Brainfuck source to compile")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("output")
                .short("o")
                .long("output")
                .value_name("FILE.wasm")
                .help("The WebAssembly output file")
                .takes_value(true)
                .required(true),
        )
        .get_matches();

    let input_path = matches.value_of_os("input").unwrap();
    let bf = fs::read(input_path).context(ErrorKind::Io)?;

    let output_path = matches.value_of_os("output").unwrap();

    // Construct a Walrus module.
    let config = ModuleConfig::new();
    let mut module = Module::with_config(config);

    let putc_type = module.types.add(&[ValType::I32], &[]);
    let getc_type = module.types.add(&[], &[ValType::I32]);
    let main_func_type = module.types.add(&[], &[]);

    let mut builder = FunctionBuilder::new();
    let (memory, _) = module.add_import_memory("env", "memory", false, 0, None);
    let pointer = module.locals.add(ValType::I32);
    let p = builder.local_get(pointer);
    let zext_u8 = LoadKind::I32_8 {
        kind: ExtendedLoad::ZeroExtend,
    };
    let one_byte = walrus::ir::MemArg {
        align: 1,
        offset: 0,
    };
    let context = BfContext {
        memory: memory,
        putc_func: module.add_import_func("env", "putc", putc_type).0,
        getc_func: module.add_import_func("env", "getc", getc_type).0,
        one_byte: one_byte,
        pointer: pointer,
        zero: builder.i32_const(0),
        one: builder.i32_const(1),
        p: p,
        at_p: builder.load(memory, zext_u8, one_byte, p),
    };

    let mut block = builder.block(Box::new([]), Box::new([]));
    let zero_p = block.local_set(context.pointer, context.zero);
    block.expr(zero_p);
    context.build(&bf, &mut block, true)?;
    let block_id = block.id();
    drop(block);
    let begin = From::from(block_id);

    let main_func = builder.finish(main_func_type, vec![], vec![begin], &mut module);
    module.exports.add("main", main_func);

    let wasm = module.emit_wasm().context(ErrorKind::Ice)?;
    fs::write(output_path, wasm).context(ErrorKind::Io)?;

    Ok(())
}
