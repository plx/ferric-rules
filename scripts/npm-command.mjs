export function npmInvocation(platform = process.platform) {
  if (platform === "win32") {
    return { command: "npm.cmd", shell: true };
  }
  return { command: "npm", shell: false };
}
