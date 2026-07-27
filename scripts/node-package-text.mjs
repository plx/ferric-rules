export function findNonLfTextFiles(files) {
  return files
    .filter(({ contents }) => contents.includes("\r"))
    .map(({ path }) => path);
}
