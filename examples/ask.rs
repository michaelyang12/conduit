//! `CONDUIT_PROVIDER=ollama cargo run --example ask -- "what is a monad, in one sentence?"`

use conduit::Conduit;

#[tokio::main]
async fn main() {
    let input = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let llm = match Conduit::load("conduit-example") {
        Ok(llm) => llm,
        Err(e) => return eprintln!("error: {e}"),
    };
    match llm
        .prompt(input)
        .system("Answer in one short paragraph.")
        .send()
        .await
    {
        Ok(r) => {
            println!("{}", r.text);
            eprintln!(
                "\n[{} / {} / {:?} / {:.1}s]",
                r.provider,
                r.model,
                r.stop,
                r.elapsed.as_secs_f32()
            );
        }
        Err(e) => eprintln!("error: {e}"),
    }
}
