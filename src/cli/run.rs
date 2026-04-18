use crate::cli::RunArgs;

impl RunArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        println!("{:?}", self.command);
        Ok(())
    }
}
