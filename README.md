# Rust Playground

This is the home of the [Tari Rust Playground][rustpen], hosted by [Tari Labs], forked from the [original] Rust
playground project.

## What's it do?

The playground allows you to experiment with Rust before you install it locally, or in any other case where you might
not have the compiler available.

It has a number of features, including:

1. A nice, unobtrusive editor with syntax highlighting.
1. The ability to compile in debug or release mode against the current stable, beta, or nightly version of Rust.
1. The top 100 popular crates (ranked by all-time downloads) and their dependencies are available for use. Just add
   `extern foo` to use them!
1. The ability to quickly load and save your code to a GitHub [Gist] and share it with your friends.
1. [rustfmt] and [Clippy] can be run against the source code.
1. The ability to see the LLVM IR, assembly, or Rust MIR for the source code.

## Architecture

A [React] frontend communicates with an [Iron] backend. [Docker] containers are used to provide the various
compilers and tools as well as to help isolate them.

When the API receives some source code, the base docker container spins up an ephemeral docker container containing the
rust compiler the compile and run the provided source code. Thus the code is sandboxed from the rest of the system.

## Resource Limits

### Network

There is no network connection between the compiler container and the outside world.

### Memory

The amount of memory the compiler and resulting executable use is limited by the container.

### Execution Time

The total compilation and execution time is limited by the container.

### Disk

This sandbox **does not** provide any disk space limits. It is suggested to run the server such that the temp directory
is a space-limited. One bad actor may fill up this shared space, but it should be cleaned when that request ends.

## Security Hall of Fame

A large set of thanks go to those individuals who have helped by reporting security holes or other attack vectors
against the Playground. Each report helps us make the Playground better!

* Preliminary sandbox testing (PID limit) by Stefan O'Rear.

If you'd like to perform tests that you think might disrupt service of the Playground, get in touch and we can create an
isolated clone toperform tests on! Once fixed, you can choose to be credited here.

## Deployment

### Amazon EC2 (Amazon Linux)

Here's an example session. This could definitely be improved and automated.

#### First-time setup (as root)

This configuration only needs to be run once.

```
apt update -y
apt install -y docker git

# Use a production-quality storage driver that doesn't leak disk space
vim /etc/sysconfig/docker
# Add to OPTIONS: --storage-driver=overlay

# Allow controlling the PID limit
vim /etc/cgconfig.conf
# Add:    pids       = /cgroup/pids;

service docker start
usermod -a -G docker ec2-user

fallocate -l 1G /swap.fs
chmod 0600 /swap.fs
mkswap /swap.fs

# Set aside disk space
fallocate -l 512M /playground.fs
device=$(losetup -f --show /playground.fs)
mkfs -t ext3 -m 1 -v $device
mkdir /mnt/playground

# Configure disk mountpoints (as root)
cat >>/etc/fstab <<EOF
/swap.fs        none            swap   sw       0   0
/playground.fs /mnt/playground  ext3   loop     0   0
EOF
```

Reboot the instance at this point.

#### Get the code
```
git clone https://github.com/tari-labs/rust-playground.git
cd rust-playground

# Build the containers
cd compiler/
./build.sh
cd ..
```

#### Set a crontab to rebuild the containers
```
crontab -e
```

```
0 0 * * * cd /home/ec2-user/rust-playground/compiler && ./build.sh
0 * * * * docker images -q --filter "dangling=true" | xargs docker rmi
```

#### Build the UI frontend and backend
```
cd ui
./build.sh
cd ..
```

#### Run the server
```
docker run -it --rm \
    --volume /var/run/docker.sock:/var/run/docker.sock \
    --volume /mnt/playground:/mnt/playground \
    --publish 80:80 \
    playground
```

## Development

### Build the UI
```
cd ui/frontend
yarn
yarn run watch # Will rebuild and watch for changes
```

If you don't need the backend running because you are only making basic HTML/CSS/JS changes, directly open in your
browser the built `ui/frontend/build/index.html`.

### Build and run the server
```
cd ui
RUST_LOG=ui=debug \
PLAYGROUND_UI_ROOT=$PWD/frontend/build/ \
cargo run
```

### Build the containers
```
cd compiler
./build.sh
cd ..
```

## Updating Rust playground

If you've edited `top-crates/crate-modifications.yml`:

```
$ cd top-crates
$ cargo update
$ cargo run
```

Then

```
$ cd compiler
$ PERFORM_PUSH=true ./build.sh
```

SSH into box and `./update.sh`


## License

Licensed under either of
 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
at your option.

[rustpen]: https://rustpen.tari.com/
[tari labs]: https://tari.com
[original]: https://github.com/integer32llc/rust-playground
[gist]: https://gist.github.com/
[rustfmt]: https://github.com/rust-lang-nursery/rustfmt
[clippy]: https://github.com/Manishearth/rust-clippy
[react]: https://facebook.github.io/react/
[iron]: http://ironframework.io/
[docker]: https://www.docker.com/