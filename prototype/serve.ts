// tiny static server for local verification of the built prototype
const root = new URL("../docs/", import.meta.url).pathname;
Bun.serve({
  port: Number(process.env.PORT ?? 8765),
  async fetch(req) {
    let p = new URL(req.url).pathname;
    if (p.endsWith("/")) p += "index.html";
    const f = Bun.file(root + p.replace(/^\//, ""));
    return (await f.exists()) ? new Response(f) : new Response("404", { status: 404 });
  },
});
console.log("serving docs/ on :" + (process.env.PORT ?? 8765));
