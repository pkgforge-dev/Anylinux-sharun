use std::{
	env,
	process::{Command, exit},
	os::unix::process::CommandExt,
};


pub fn run(exec_args: &[String]) -> ! {
	env::set_var("GIO_LAUNCHED_DESKTOP_FILE_PID", std::process::id().to_string());

	let err = Command::new(&exec_args[0])
		.args(&exec_args[1..])
		.exec();
	eprintln!("gio-launch-desktop: failed to exec '{}': {err}", exec_args[0]);
	exit(1)
}
