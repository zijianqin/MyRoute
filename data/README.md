# Map data

Place user-supplied regional OpenStreetMap `.osm.pbf` extracts in this directory. Source extracts and generated `.myroute` graphs are ignored by Git and are not distributed with MyRoute.

For v0.1, use a small Princeton-area or New Jersey extract rather than a planet file. Import it with:

```bash
myroute import data/new-jersey.osm.pbf --output data/new-jersey.myroute
```

OpenStreetMap data is © OpenStreetMap contributors and is available under the Open Database License. Follow the provider's download policy and preserve attribution in maps and derivative uses. The generated HTML viewer includes OpenStreetMap attribution.
