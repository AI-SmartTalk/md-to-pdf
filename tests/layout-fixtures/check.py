#!/usr/bin/env python3
"""Banc d'essai du Layout Doctor.

Chaque fichier .md de ce dossier provoque volontairement UN defaut. Le banc verifie deux
choses, et les deux comptent autant : que l'analyseur trouve ce defaut, et qu'il ne trouve
RIEN d'autre. Un faux positif discredite la fonctionnalite entiere, donc un defaut detecte
en trop echoue le banc au meme titre qu'un defaut manque.

Lancement : voir run.sh, qui demarre le service et appelle ce script.
"""

import json
import os
import sys
import urllib.error
import urllib.request

BASE = os.environ.get("LAYOUT_BENCH_URL", "http://127.0.0.1:8011")
KEY = os.environ.get("API_KEY", "layout-bench")
HERE = os.path.dirname(os.path.abspath(__file__))

# fixture -> (defauts attendus, le correctif doit-il ameliorer le score)
EXPECTED = {
    "clean": (set(), False),
    "wide-table": ({"overflow"}, True),
    "long-url": ({"overflow"}, True),
    "widow-page": ({"widow_page"}, True),
    "orphan-heading": ({"orphan_heading"}, True),
    "split-table": ({"split_table"}, True),
    # Une page vide ne se corrige pas par du CSS : elle se signale, c'est tout.
    "blank-page": ({"blank_page"}, False),
}


def post(path, payload):
    request = urllib.request.Request(
        BASE + path,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "X-API-Key": KEY},
    )
    try:
        with urllib.request.urlopen(request, timeout=180) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as error:
        sys.exit(f"{path} a repondu {error.code} : {error.read().decode()[:300]}")


def render(name, markdown, css, autolayout):
    payload = {
        "markdown": markdown,
        "client_id": "layout-bench",
        "pdf_name": f"{name}-{'fixed' if autolayout else 'raw'}.pdf",
        "options": {"autolayout": autolayout} if autolayout else {},
    }
    if css:
        payload["css"] = css
    return post("/api/convert", payload)


def custom_css(name):
    """CSS du client, quand la fixture en a un.

    La feuille par defaut du service protege deja les tableaux et les titres. Deux fixtures
    fournissent donc un CSS qui leve cette protection : sans cela le defaut ne se produit
    pas, et c'est justement le cas ou le Layout Doctor sert a quelque chose.
    """
    path = os.path.join(HERE, name + ".css")
    if not os.path.exists(path):
        return None
    with open(path) as source:
        return source.read()


def main():
    failures = []

    for name, (expected, must_improve) in EXPECTED.items():
        with open(os.path.join(HERE, name + ".md")) as source:
            markdown = source.read()

        css = custom_css(name)

        plain = render(name, markdown, css, False)
        report = post("/api/layout", {"pdf": plain["download_url"]})
        found = {issue["kind"] for issue in report["issues"]}

        fixed = render(name, markdown, css, True)
        after = fixed.get("layout", {})

        line = (
            f"{name:<16} pages={report['pages']:<3} score={report['score']:<4}"
            f" -> {after.get('score', '?'):<4} passes={after.get('passes', '?')}"
            f" [{', '.join(sorted(found)) or 'rien'}]"
        )
        print(line)
        for issue in report["issues"]:
            print(f"    {issue['kind']:<15} p{issue['page']} {issue['severity']:<6} {issue['detail']}")

        if found != expected:
            failures.append(f"{name}: attendu {sorted(expected)}, trouve {sorted(found)}")
        if must_improve and after.get("score", 0) <= report["score"]:
            failures.append(
                f"{name}: le correctif n'ameliore pas le score "
                f"({report['score']} -> {after.get('score')})"
            )
        # Un second passage identique doit rendre exactement le meme score
        again = post("/api/layout", {"pdf": plain["download_url"]})
        if again != report:
            failures.append(f"{name}: deux analyses du meme PDF ne donnent pas le meme rapport")

    if failures:
        print("\nECHECS :")
        for failure in failures:
            print("  - " + failure)
        return 1

    print("\nBanc d'essai vert.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
