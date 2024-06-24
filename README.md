# Lemmy Thumbnail Cleaner

This is a simple program to remove old thumbnails from [pict-rs](https://git.asonix.dog/asonix/pict-rs)
and [lemmy](https://github.com/LemmyNet/lemmy).

It will periodically check the lemmy database for posts that are older than given amount of months and instruct pict-rs
to drop the thumbnail for that post.

## Usage

This program requires connection to the lemmy postgres database and pict-rs HTTP service.
The expected deployment is as container/service alongside the pict-rs and lemmy postgres services.

Edit the lemmy `docker-compose.yml` to include this service:

```yaml
services:

  # ....

  cleaner:
    image: ghcr.io/wereii/lemmy-thumbnail-cleaner:v0.1.2
    #restart: unless-stopped
    environment:
      - RUST_LOG=info
      - INSTANCE_HOST=https://your_instance_host.here/
      - POSTGRES_DSN=postgresql://user:password@postgres/lemmy
      - PICTRS_HOST=pict-rs:8080
      - PICTRS_API_KEY=XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
      #- THUMBNAIL_MIN_AGE_MONTHS=3
      #- CHECK_INTERVAL=300
      #- QUERY_LIMIT=100
```

⚠️ **Only pict-rs 0.5+ can be used, older versions do not implement required API endpoints!** ⚠️

Pict-rs also needs to be configured with api key (`PICTRS__SERVER__API_KEY`), otherwise the endpoint required for this cleaner is not accessible!


## Configuration

#### Required Environment Variables

- `INSTANCE_HOST` - The "root url" of your lemmy instance. For `lemmy.world` this would look
  like `https://lemmy.world/`.

  This is used to determine if a thumbnail of a post is local to this instance (and thus can be deleted).

- `POSTGRES_DSN` - The URI to the lemmy postgres database. Must be full postgres DSN and must specify the lemmy
  database (usually `/lemmy`).

- `PICTRS_API_KEY` - The API key for the pict-rs service.

- `PICTRS_HOST` - The host of the pict-rs service, this is just ip/hostname, optionally port (ex. `pict-rs:8080`).

#### Optional Environment Variables

- `RUST_LOG` - Controls logging level, `debug` will also print the thumbnail ids being deleted (lots of lines).  
  Without this the default level is `warn`.

- `THUMBNAIL_MIN_AGE_MONTHS` - The minimum age of a thumbnail in months before it is considered for deletion.  
  Default is `3` months.

- `CHECK_INTERVAL` - The interval in seconds the program sleeps between checks.  
  Default is `300`.
  The main use is to give other services breathing room and not keep hitting them constantly with
  requests.
    - **Setting this to `0` will make the program run once and then exit.**

- `QUERY_LIMIT` - The maximum number of posts to get from postgres in one query and thus the maximum number of
  thumbnails to delete in one check interval.  
  Default is `100`.
    - Increasing this has direct impact on postgres as it has to return more rows.
    - It will also increase memory usage of the cleaner as it keeps the rows in memory until processed (though this
      shouldn't have too big of an impact).

- `DELETE_ON_NOT_FOUND` - If set to `true` the cleaner will "unlink" the thumbnail from the post even if pict-rs returns 404 (not found)
  when trying to delete it (basically on 404 it assumes the image does not exist already).
  Default is `false`.
    - **Warning: This can leave "dead" thumbnails in pict-rs that are not associated with any post!**
    - You should only set this if you are sure that the thumbnail can't be anywhere else (e.g. in some other pict-rs
      instance).

The `CHECK_INTERVAL` and `QUERY_LIMIT` is what controls how demanding the cleaner is on the database and pict-rs.
You should tweak it to fit the performance of your infrastructure.

When there is a lot (10k+) that can be cleaned up you should reduce the `CHECK_INTERVAL` (5-15s) and then
increase `QUERY_LIMIT` (~500) to speed up the process.
Keep in mind the program is intentionally single-threaded so increasing `QUERY_LIMIT` too much will keep the program
continually hitting
both pict-rs and postgres for longer.

Once there is less (hundreds) you can increase the `CHECK_INTERVAL` to hours or days as there won't be that much new
thumbnails old enough (but that depends on your traffic).  
I would personally expect this to run once or twice a day at that point, with query limit of around the 300.

# Notes
 
####  Backblaze B2

When the bucket lifecycle is configured to `Keep Only Last Version`, the [old versions are not deleted
  immediately but hidden instead](https://www.backblaze.com/blog/backblaze-b2-lifecycle-rules/) and deleted after 24h.  
  So don't be surprised if the bucket size doesn't change immediately.

### Results:

- My instance of 2 MAU, running for about half a year had about 95k files and 12G of data before running the cleaner.
- After running the cleaner, fully removing all older then 1 month (and after waiting for almost 2 days) the b2 bucket usage dropped to 57k files and 7.9G
 
# Disclaimer

My rust is _rusty_ so there might be some issues with the code.   
I have tested this on my own instance and it works as expected but

**USE AT YOUR OWN RISK**

