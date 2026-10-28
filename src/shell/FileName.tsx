/** A file name whose extension stays at its end: "فيلا.dwg", never "dwg.فيلا". A single `<bdi>`
 *  takes an Arabic name's direction, which puts the Latin extension before it. */
export function FileName({ name }: { name: string }) {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || !/^\.[A-Za-z0-9$]{1,8}$/.test(name.slice(dot))) return <bdi>{name}</bdi>;
  return <bdi dir="ltr"><bdi>{name.slice(0, dot)}</bdi>{name.slice(dot)}</bdi>;
}
