export type Route =
  | { screen: "home" }
  | { screen: "editor"; projectId: string };

function parse(hash: string): Route {
  const m = /^#\/project\/([^/?#]+)/.exec(hash);
  if (m && m[1]) return { screen: "editor", projectId: decodeURIComponent(m[1]) };
  return { screen: "home" };
}

class Router {
  route = $state<Route>({ screen: "home" });

  constructor() {
    this.route = parse(window.location.hash);
    window.addEventListener("hashchange", () => {
      this.route = parse(window.location.hash);
    });
  }

  navigate(hash: string): void {
    if (window.location.hash === hash) {
      this.route = parse(hash);
    } else {
      window.location.hash = hash;
    }
  }

  goHome(): void {
    this.navigate("#/");
  }

  openProject(id: string): void {
    this.navigate(`#/project/${encodeURIComponent(id)}`);
  }
}

export const router = new Router();
