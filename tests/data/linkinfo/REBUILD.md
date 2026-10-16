# Rebuilding the archive the parity lane uses

`x86_64-unknown-linux-gnu.3.7.1-viprs.2.json` is the `metadata/LINKINFO.json` of an archive built from libviprs-dep `bc351a906d9085f256fb2226fd6050f72e2d3325`, and it is committed here as that lane's provenance. `docs/AOT_PARITY.md` has the results.

The recipe, run on a native x86_64 docker host (MARS):

```sh
# the tree at that commit, without .git
git -C libviprs-dep archive bc351a9 acadsharp | tar -x -C dep-bc351a9

# the driver runs `docker build`, `docker create` and `docker cp` itself, so the
# container gets the host's docker socket and a docker CLI (nas.sh `build` does
# this; on a rootless host the socket is the user's, not /var/run/docker.sock)
docker run --rm --platform linux/amd64 \
  -v <docker.sock>:/var/run/docker.sock \
  -v $PWD/dep-bc351a9:/work -w /work nas-driver:latest \
  sh -c 'python3 acadsharp/build_acadsharp.py --platform linux --arch amd64'

# the archive lands in dep-bc351a9/bin/acadsharp-linux-x64.tgz
```

Then unpack it and point `ACADSHARP_NATIVE_DIR` at the single top-level directory inside. Without `.git` the driver cannot read its own commit, so `BUILDINFO.json` says `driver_commit: unknown`; the commit above is the label.

The build took about an hour on MARS, nearly all of it the builder image's package installs.
