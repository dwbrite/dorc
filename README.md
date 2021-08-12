TODO: add badges

# Devin's Orchestrator (`dorc`) - a stupid deployment utility

`dorc` is a tool for deploying simple backend services with a green–blue strategy.


## Requirements, warnings, et al

`dorc` as it stands will only function on linux systems that use SystemD.
That's pretty much the only requirement.


### Not all software will work with `dorc`!

Binaries need to have a way to set which port they listen on (e.g., `./yourbin --port 8081`)

You may run into trouble if your software uses filesystem as permanent storage if that data is stored relative to the working directory.

That means you should be storing data in an external database, or a bucket, or in some absolute path like `/etc/yourapp/data`.

---

If you need more (or different) functionality, use a more mainstream deployment tool like k8s or docker swarm.


## Understanding `dorc`

To understand more about `dorc`, we need to talk terminology.

An _application_ includes a release directory and a release binary.
This is the location you should upload files for your software.

Applications also consist of _services_ (also referred to as subservices).
A service is a living version of your software.
Each application has two subservices: `green-{app}` and `blue-{app}`,
only one of which receives traffic at a given moment.
That is to say, one is considered _active_ and the other is _inactive_.
`dorc` services are also registered as _SystemD services_.

The _daemon_ is a background process that:
- routes traffic from an application's listen port to the current active service,
- watches release files to keep the inactive service up-to-date,
- listens for commands to load, update, and remove applications;
  and reload or swap the active service of an application.

---

With that out of the way, let's talk about how everything works in practice.

First you should configure your CI/CD workflow to upload a release to your server.
You can see how I do that for my website [here](https://github.com/dwbrite/website-rs/blob/master/.github/workflows/dwbrite-com.yml).

[Install `dorc`](#installing-dorc) on your server.
It will be registered as a _SystemD service_ that starts the daemon on boot.
If SystemD neglects to start the daemon on install, just run `systemctl start dorc`.

You can run `dorc register` to register an application and its subservices.

Once you've uploaded a new version of your software, `dorc` will copy that to the inactive service.
Then you can call `dorc switch {app}` to swap which subservice is considered active.
If you have any problems, simply call `dorc switch {app}` again to roll-back to the previous version.

Happy deploying!


## Installing `dorc`

If I've uploaded this to crates.io, you can probably just run `cargo install dorc`.
Otherwise, you may need to clone the repo and install `dorc` manually, along with the SystemD service file.
(On debian-based distros, I've already set up `cargo-deb` to help with this!)
