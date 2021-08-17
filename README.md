TODO: add badges

```
    __/\\\\\\\\\\\\__________/\\\\\_________/\\\\\\\\\____________/\\\\\\\\\_
     _\/\\\////////\\\______/\\\///\\\_____/\\\///////\\\_______/\\\////////__
      _\/\\\______\//\\\___/\\\/__\///\\\__\/\\\_____\/\\\_____/\\\/___________
       _\/\\\_______\/\\\__/\\\______\//\\\_\/\\\\\\\\\\\/_____/\\\_____________
        _\/\\\_______\/\\\_\/\\\_______\/\\\_\/\\\//////\\\____\/\\\_____________
         _\/\\\_______\/\\\_\//\\\______/\\\__\/\\\____\//\\\___\//\\\____________
          _\/\\\_______/\\\___\///\\\__/\\\____\/\\\_____\//\\\___\///\\\__________
           _\/\\\\\\\\\\\\/______\///\\\\\/_____\/\\\______\//\\\____\////\\\\\\\\\_
            _\////////////__________\/////_______\///________\///________\/////////__
   
                              A stupid deployment utility!
```


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

Say you're Devin W. Brite. You host your website (dwbrite.com) on a Raspberry Pi or a VPS.
Your website is a self-hosted binary with a working directory, hosted on port 8081 with nginx sitting in front.
You want to deploy new versions of your website without any downtime. What do you do?

You _could_ run two versions of your site on your machine and then 
switch between them by enabling and disabling configurations. 
But that feels clunky and you technically have some downtime. 
_And_ when your binaries change you'll have to stop and restart them yourself.

Setting up k8s is a pain and k8 engines are _annoying_. There's a few other high friction options, but you're _lazy_.

If that sounds like you, `dorc` is your solution in search of a problem!

### What does `dorc` _do_?

`dorc` helps you set up two live versions of your application. Then it functions as a simple proxy. \
Let's call a live version of an application a _service_, so we have a blue service and a green service.\
Both services run at the same time, but only one is routed to at a time. Thus we have an active and an inactive service.

The `dorc` _daemon_ will listen on a port you specify, and forward data to the port of the active version. \
When you upload a new release, `dorc` copies the files to the _inactive_ version and restart the binary.

Once you're ready to show the new version of your application to the world,
you can run `dorc switch {my-app}` and not miss a beat. \
Old connections are kept alive and new connections are routed to the other service.

When you register an application with `dorc`, the blue and green services will be registered with `SystemD`. \
This allows you to manage sub-services and be sure that your processes are kept alive.


### `dorc` by example

I have Github Actions [upload releases to my server](https://github.com/dwbrite/website-rs/blob/master/.github/workflows/dwbrite-com.yml).
Note that I upload my files to `/var/tmp/dwbrite.com/`.

Then I run `dorc register`. \
I'll call my application `dwbrite.com` and tell `dorc` to listen on port `41234`. \
I'll also tell it that the working directory is `/var/tmp/dwbrite.com`, 
and that the binary is at `/var/tmp/dwbrite.com/target/dwbrite.com`.

Then I need to set up `green-dwbrite.com` and `blue-dwbrite.com`

I leave `Working dir:` default, and tell `dorc` how to start and stop my application.

And that's it!

`dorc` is pretty stupid, so if I want to run more than one website, I need to run `nginx` in front of it.

```
             apache/nginx                                 blue-dwbrite.com
            ┌─────────────────┐                          ┌─────────────────┐
            │                 │                          │.        .   *   │
            │   dwbrite.com───►:41234                    │   .      .      │
:80,:443◄───►                 │              :41235◄─────►  .     *    . * │
            │                 │                          │  *    .  *   :) │
            │   ohej.us───────►:8080                     │     ..    *    .│
            │                 │                          │   *    *    .   │
            └─────────────────┘                          └─────────────────┘




             dorc (dwbrite.com)                           green-dwbrite.com
            ┌─────────────────┐                          ┌─────────────────┐
            │       active    │                          │   *    ..      .│
            │     ┌───────────►:41235                    │.            *   │
:41234◄─────┼─────┘           │              :41236◄─────►  :) *    .    . │
            │     . inactive  │                          │* .    .  *   .  │
            │     .............:41236                    │    *  .  ..  *  │
            │                 │                          │  .      *   *  .│
            └─────────────────┘                          └─────────────────┘
```

When a new version of my website is uploaded, `dorc` will copy that to the inactive service. \
Then I can call `dorc switch dwbrite.com` to swap which service is considered active. \
If I run into any problems, or if I simply don't _like_ this change, 
I can call `dorc switch dwbrite.com` again to roll back to the previous version.

---

![example image](https://github.com/dwbrite/dorc/blob/master/meta/screenshot.png?raw=true)

---


Happy deploying!


## Installing `dorc`

`dorc` relies on Nightly Rust and `cargo deb`:  
- `rustup toolchain install nightly`
- `cargo install cargo-deb`

Clone the project, `cd` into it, and run `cargo +nightly deb --install`