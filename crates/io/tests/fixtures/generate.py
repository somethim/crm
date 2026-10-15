#!/usr/bin/env python3
"""Writes the synthetic sample import files ARTIKUJ.csv, KLIENTET.csv and FURNITORET.csv.

Every name, address, NIPT, account code and price is invented. The files keep the
layout of the wholesaler's Bilanc exports (headers, UTF-8 without a BOM, LF line
endings, minimal quoting, padded NIPTs, shared NIPTs, flavour codes, ...) and their
row counts (776 products, 2931 clients, 186 suppliers), so the import, search and
performance tests stay meaningful. The rows the tests look up by value are planted
explicitly below (search the tests for the same strings).

Deterministic: `python3 crates/io/tests/fixtures/generate.py` rewrites the three
files byte for byte. Standard library only.
"""

import csv
import random
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
rng = random.Random(20261015)


def write(name, header, rows):
    with open(HERE / name, "w", encoding="utf-8", newline="") as f:
        out = csv.writer(f, lineterminator="\n", quoting=csv.QUOTE_MINIMAL)
        out.writerow(header)
        out.writerows(rows)


# ---------------------------------------------------------------- invented words

ONSETS = ["B", "D", "DR", "F", "G", "GJ", "K", "L", "M", "N", "P", "R", "S", "SH", "T", "V", "Z", "XH", "H", "J"]
VOWELS = ["A", "E", "I", "O", "U", "A", "E", "I", "O", "Ë"]
CODAS = ["", "", "", "N", "R", "L", "S", "T", "M"]


def syllable():
    return rng.choice(ONSETS) + rng.choice(VOWELS) + rng.choice(CODAS)


def near(a, b):
    """True when `a` and `b` are within edit distance 2 (cheap check for short words)."""
    if abs(len(a) - len(b)) > 2:
        return False
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1] <= 2


# Words the search tests rely on: no generated word may come near them.
RESERVED = ["BRIZO", "BRYZO", "BELARIS", "KORABI", "PJESHKE", "MORENA", "GRISINA", "LIVADHET", "YJE"]
used_words = set()


def word(min_syl=2, max_syl=3, ascii_only=False):
    while True:
        w = "".join(syllable() for _ in range(rng.randint(min_syl, max_syl)))
        if ascii_only:
            w = w.replace("Ë", "E")
        if len(w) < 5 or w in used_words or w.startswith("BR"):
            continue
        if any(near(w, r) for r in RESERVED):
            continue
        used_words.add(w)
        return w


# ---------------------------------------------------------------- prices

def buy_text(value):
    """A buy price as Bilanc writes it: mostly whole lek, sometimes a long fraction."""
    roll = rng.random()
    if roll < 0.86:
        return str(int(round(value)))
    if roll < 0.92:
        return f"{value:.1f}".rstrip("0").rstrip(".") if value < 20 else str(int(value)) + "." + str(rng.randint(1, 9))
    digits = rng.choice([3, 4, 9, 11, 12, 13])
    text = f"{value:.{digits}f}".rstrip("0")
    return text if not text.endswith(".") else text[:-1]


def sell_for(buy):
    return int(round(buy * rng.uniform(1.04, 1.3)))


# ---------------------------------------------------------------- ARTIKUJ.csv

SIZES = {
    "A": ["0.7 LIT", "6*0.7 LIT", "1L", "6* 1 LIT", "0.5LIT", "12*1 L", "0.75 LIT", "6X0.7 L", "6* 0.75 LIT", "0.7  LIT"],
    "B": ["24*0.5 (KANACE)", "24*0.33 LIT (SHISHE)", "24* 0.33LIT (SHISHE)", "24*0.5", "20*0.5 LIT (SHISHE)"],
    "W": ["0.75 LIT", "6*0.75 LIT", "5 LIT", "3 LIT (BAG IN BOX)", "0.187 LIT"],
    "F": ["24*0.33ML", "6*1.5 LIT", "24*0.33 LIT", "12*0.5 LIT", "24* 0.25 LIT"],
    "U": ["12*0.5 LIT", "6*1.5 LIT", "12X0.33", "24*0.25 LIT", "12 * 0.5 LIT"],
}
ALCOHOL = ["VODKA", "WHISKY", "WHISKEY", "GIN", "RUM", "LIKER", "KONJAK", "RAKI", "TEKILA", "VERMUT", "APERITIV", "BRANDY", "SHAMPANJE", "PROSECCO", "GRAPPA"]
EXTRAS = ["", "", "", "", "(12 VJECAR)", "(KREM)", "(BLACK)", "(DHURATE)", "(SPICED)", "(15 YEARS)"]
WINE = ["E KUQE", "E BARDHE", "ROZE", "RESERVA", "MERLOT", "SHESH I ZI", "KALLMET", "SHESH I BARDHE"]
FRUITS = ["MOLLE", "DARDHE", "PORTOKALL", "ANANAS", "MULTIVITAMINE", "VISHNJE", "KAJSI", "BOROVINE", "SHEGE"]
SNACKS = ["CAMCAKEZ", "COKOLLATE", "VAFER", "KARAMELE", "FARA", "BAR PROTEINIK"]

# (prefix, rows, generator) in file order; planted rows are added separately.
PLAN = [
    ("A", 318), ("AS", 39), ("B", 65), ("BIS", 20), ("COK", 30), ("D", 48), ("E", 3),
    ("F", 38), ("K", 9), ("KEK", 6), ("KRM", 31), ("L", 20), ("M", 3), ("MON", 6),
    ("P", 14), ("Q", 5), ("U", 17), ("USH", 44), ("V", 7), ("W", 53),
]

PLANTED = {
    # The first row (import and export tests).
    "A0002": ("LIKER MORENA 0.5LIT (LIQUER)", "SHISHE", "6", "1080", "1200"),
    # Transposition and typo examples (search tests: "amaor korabi", "belris").
    "A0008": ("AMARO KORABI 6X0.7 L", "SHISHE", "52", "1090", "1200"),
    "A0009": ("BELARIS 6X0.7 L (KREM LIKER)", "SHISHE", "18", "990", "1200"),
    # Decimal buy and sell prices.
    "COK0024": ("FARA KUNGULLI (6*50CP)", "COPE", "412", "7.4", "7.9"),
    # The one KG product, with a quantity of 500.
    "KRM0129": ("SHEQER KRM 10 KG - 5G", "KG", "500", "95", "140"),
    # A base code with five flavours ("BRIZO" is the brand the ranking tests use).
    "L0009": ("BRIZO 24*0.33LIT", "COPE", "3840", "58", "62"),
    "L0009/1": ("BRIZO PJESHKE 24*0.33LIT", "COPE", "0", "0", "62"),
    "L0009/2": ("BRIZO MOLLE 24*0.33LIT", "COPE", "0", "0", "62"),
    "L0009/3": ("BRIZO LULE 24*0.33LIT", "COPE", "0", "0", "62"),
    "L0009/4": ("BRIZO RRUSHI 24*0.33LIT", "COPE", "0", "0", "62"),
    "L0009/5": ("BRIZO LULE & BANANE 24*0.33LIT", "COPE", "0", "0", "62"),
    "L0010": ("BRIZO  6*1.5LIT", "COPE", "214", "118", "130"),
    "L0011": ("BRIZO ZERO 12*0.33ML", "COPE", "96", "61", "70"),
    "L0016": ("BRYZO ENERGJI 24*0.25LIT", "COPE", "48", "71", "80"),
    "L0017": ("BRYZO ENERGJI MANGO 24*0.25LIT", "COPE", "24", "71", "80"),
    # Four more PJESHKE products without BRIZO.
    "L0021": ("LENA PJESHKE 1*24", "COPE", "640", "29", "33"),
    "F0045": ("CAJ I FTOHTE DRITA PJESHKE 24*0.33ML", "COPE", "820", "44.2715", "48"),
    "USH0031": ("JOGURT PJESHKE 12*150GR", "COPE", "36", "61", "70"),
    "USH0032": ("KOMPOSTO PJESHKE 0.8 KG", "COPE", "24", "185", "210"),
    # A base code with a three-decimal buy price and five flavours.
    "P0035": ("GRISINA 12*70 GR", "COPE", "988", "47.383", "55"),
    "P0035/1": ("GRISINA PICA 12* 70 GR", "COPE", "0", "0", "55"),
    "P0035/2": ("GRISINA HUDHRA 12* 70 GR", "COPE", "0", "0", "55"),
    "P0035/3": ("GRISINA NATYRAL 12* 70 GR", "COPE", "0", "0", "55"),
    "P0035/4": ("GRISINA SUSAM 12* 70 GR", "COPE", "0", "0", "55"),
    "P0035/5": ("GRISINA ROZMARINE 12* 70 GR", "COPE", "0", "0", "55"),
}


def product_name(prefix):
    b = word(ascii_only=True)
    if prefix == "A":
        kind = rng.choice(ALCOHOL)
        name = f"{kind} {b} {rng.choice(SIZES['A'])} {rng.choice(EXTRAS)}".strip()
        return name, (rng.uniform(600, 9000) if rng.random() < 0.9 else rng.uniform(9000, 30000))
    if prefix == "AS":
        thing = rng.choice(["GOTA PLASTIKE", "PECETA", "KALLAMA", "MBULESA", "PJATA LETRE", "LUGE PLASTIKE", "SHKOP DHEMBESH"])
        return f"{thing} {b} ({rng.choice([50, 100, 200, 500])} CP)", rng.uniform(15, 400)
    if prefix == "B":
        return f"BIRRE {b} {rng.choice(SIZES['B'])}", rng.uniform(45, 260)
    if prefix == "BIS":
        return f"BISKOTA {b} {rng.choice([12, 24, 30])}*{rng.choice([40, 100, 200])}GR", rng.uniform(20, 180)
    if prefix == "COK":
        return f"{rng.choice(SNACKS)} {b} (1*{rng.choice([20, 24, 30])}CP)", rng.uniform(8, 90)
    if prefix == "D":
        thing = rng.choice(["DETERGJENT", "SAPUN LENGSHEM", "ZBUTES RROBASH", "LETER HIGJENIKE", "SHAMPO", "ZBARDHUES"])
        return f"{thing} {b} {rng.choice(['1LIT', '1.9LIT', '750 ML', '4 KG', '10 ROLE'])}", rng.uniform(90, 900)
    if prefix == "E":
        return f"ENERGJIKE {b} 24*0.25 LIT", rng.uniform(60, 110)
    if prefix == "F":
        kind = rng.choice(["LENG GAZ", "CAJ I FTOHTE", "LIMONADE", "TONIK", "SODE"])
        return f"{kind} {b} {rng.choice(SIZES['F'])}", rng.uniform(40, 130)
    if prefix == "K":
        return f"KAFE {b} {rng.choice(['100GR*30CP', '1 KG', '250 GR', '500GR'])}", rng.uniform(110, 1800)
    if prefix == "KEK":
        return f"KEK {b} {rng.choice([24, 30])}*{rng.choice([40, 50, 60])}GR", rng.uniform(15, 40)
    if prefix == "KRM":
        kind = rng.choice(["KAPUCINO", "COKOLLATE E NXEHTE", "SALEP", "KREM KAFE"])
        return f"{kind} {b} {rng.choice(['10 B', '10B', '400 GR', '1 KG'])}", rng.uniform(250, 700)
    if prefix == "L":
        return f"LENG {b} {rng.choice(FRUITS)} {rng.choice(['1*24', '1 LIT', '24*0.2 LIT', '27*0.25 LIT'])}", rng.uniform(25, 160)
    if prefix == "M":
        return f"MIELL {b} {rng.choice(['1 KG', '5 KG', '25 KG'])}", rng.uniform(80, 1500)
    if prefix == "MON":
        return f"SHURUP {b} {rng.choice(['0.7LIT (SHISHE)', '1.89LIT (BIDON PLASTIK)', '1 LIT'])}", rng.uniform(700, 1600)
    if prefix == "P":
        return f"PATATINA {b} {rng.choice(['38*40GR', '20*150 GR', '24*75GR'])}", rng.uniform(30, 160)
    if prefix == "Q":
        return f"QESE {b} {rng.choice(['E VOGEL', 'E MESME', 'E MADHE'])} ({rng.choice([50, 100])} CP)", rng.uniform(50, 300)
    if prefix == "U":
        gas = rng.choice(["", "", "ME GAZ ", "ME GAZ ME LIMON "])
        return f"UJE {b} {gas}{rng.choice(SIZES['U'])}", rng.uniform(15, 60)
    if prefix == "USH":
        thing = rng.choice(["MAKARONA", "ORIZ", "VAJ ULLIRI", "SALCE DOMATE", "TON", "ULLINJ", "DJATHE", "MAJONEZE", "KETCHUP"])
        return f"{thing} {b} {rng.choice(['500 GR', '1 KG', '1 LIT', '12*400GR', '6*0.5 LIT'])}", rng.uniform(60, 900)
    if prefix == "V":
        return f"VAJ {b} {rng.choice(['1 LIT', '5 LIT', '3 LIT'])}", rng.uniform(150, 1200)
    if prefix == "W":
        return f"VERE {b} {rng.choice(WINE)} {rng.choice(SIZES['W'])}", rng.uniform(300, 5000)
    raise ValueError(prefix)


def artikuj():
    rows = []
    planted_by_prefix = {}
    for code in PLANTED:
        prefix = re.match(r"[A-Z]+", code).group()
        planted_by_prefix[prefix] = planted_by_prefix.get(prefix, 0) + 1
    names = {v[0] for v in PLANTED.values()}
    taken_codes = set(PLANTED)
    for prefix, count in PLAN:
        need = count - planted_by_prefix.get(prefix, 0)
        free = [n for n in range(3 if prefix == "A" else 1, 600) if f"{prefix}{n:04d}" not in taken_codes]
        for number in sorted(rng.sample(free, need)):
            code = f"{prefix}{number:04d}"
            while True:
                name, value = product_name(prefix)
                if rng.random() < 0.03:
                    name = name.replace(" ", "  ", 1)
                if name not in names:
                    break
            names.add(name)
            buy = buy_text(value)
            sell = sell_for(float(buy))
            if prefix == "A":
                unit = "SHISHE" if rng.random() < 0.24 else "COPE"
            else:
                unit = "COPE"
            qty = 0 if rng.random() < 0.028 else int(rng.choice([rng.randint(1, 60), rng.randint(1, 400), rng.randint(20, 5000)]))
            rows.append((code, name, unit, str(qty), buy, str(sell)))
    rows.extend((code, *values) for code, values in PLANTED.items())
    # A few more decimal sell prices, as in the source export.
    decimal_sells = [i for i, r in enumerate(rows) if r[0].startswith(("COK", "KEK")) and r[0] not in PLANTED][:3]
    for i in decimal_sells:
        r = list(rows[i])
        r[5] = f"{int(r[5])}.5"
        rows[i] = tuple(r)
    rows.sort(key=lambda r: r[0])
    assert len(rows) == 776, len(rows)
    assert len({r[0] for r in rows}) == 776
    assert rows[0][0] == "A0002"
    write("ARTIKUJ.csv", ["Kodi", "Përshkrimi", "Njësi", "Gjendje", "CMIM BLERJE", "CMIM SHITJE"], rows)


# ---------------------------------------------------------------- owners

FIRST = [
    "ARBEN", "ARDIT", "ALBAN", "ARMANDO", "BLEDAR", "BESNIK", "DRITAN", "DORIAN", "ENDRIT", "ERION", "ERVIS",
    "FATMIR", "FLORIAN", "GENTI", "GËZIM", "ILIR", "KLODIAN", "KLEVIS", "LEDIO", "LULZIM", "MIRELA", "MARSIDA",
    "OLSI", "ORGES", "PETRIT", "SKERDI", "SOKOL", "TEUTA", "VALMIR", "XHENI", "ERJON", "ELONA", "ENGJËLL",
    "DRILON", "BLERINA", "JONIDA", "AURELA", "ARJOLA", "SAIMIR", "ROLAND", "ANDI", "KEJDI", "REDI", "ERALD",
    "BUJAR", "LINDITA", "MERITA", "AGRON", "ASTRIT", "VULLNET", "ÇLIRIM", "DËSHIRA", "SHPËTIM", "FLUTURA",
]
SURNAME_HEADS = ["BAR", "DEL", "GJO", "HAS", "KOL", "LEK", "MAL", "PRE", "SHK", "TOL", "VER", "ZEN", "KUQ", "DRI", "BUL",
                 "FER", "GRA", "MUR", "QER", "XHA", "LLE", "RRA", "ÇEL", "BRË", "DOK", "SEL", "HOX", "PEP", "TAF", "NDO"]
SURNAME_MIDS = ["A", "E", "I", "O", "U", "AR", "EN", "IM", "ËS"]
SURNAME_TAILS = ["JAJ", "RAJ", "LLARI", "NAJ", "SHI", "TI", "KU", "DANI", "METI", "LLAJ", "ZAJ", "RI", "NI", "ÇI", "KAJ", "HYSA"]
SHOPS = ["BAR", "BAR-KAFE", "KAFE", "MARKET", "MINIMARKET", "DYQANI", "RESTORANT", "PIZZERI", "FURRA", "HOTEL",
         "LOKALI", "PASTIÇERI", "ENOTEKA", "SNACK BAR", "BAR-RESTORANT"]
NOTES = ["Te ura", "Mbrapa shkollës", "Pranë parkut", "Qoshja", "Tek stacioni", "Afër tregut", "Kati 2", "Lagjja e re"]
STREET_NAMES = ["e Ullinjve", "e Pishave", "e Lulishtes", "e Kodrës", "e Burimeve", "e Fushës", "e Shelgjeve",
                "e Mullirit", "e Arave", "e Bletëve", "e Dardhave", "e Gështenjave", "e Çinarëve", "e Vreshtave"]
AREAS = ["FUSHA E MADHE", "KODRA E DIELLIT", "LAGJJA 9", "LAGJJA E PISHAVE", "ARRAT E REJA",
         "KRYQËZIMI I RI", "UDHËKRYQI", "PRANË URËS", "BREGU I KANALIT", "ZONA INDUSTRIALE", "FSHATI I RI"]

used_names = set()
used_nipts = set()


def surname():
    return rng.choice(SURNAME_HEADS) + rng.choice(SURNAME_MIDS) + rng.choice(SURNAME_TAILS)


def person():
    return f"{rng.choice(FIRST)} {surname()}"


def unique(make):
    while True:
        n = make()
        if n.upper() not in used_names:
            used_names.add(n.upper())
            return n


def client_name():
    roll = rng.random()
    if roll < 0.78:
        return person()
    if roll < 0.95:
        return f"{rng.choice(SHOPS)} {word()}"
    if roll < 0.985:
        first, last = rng.choice(FIRST).title(), surname().title()
        return f"{first} {last} ({rng.choice(NOTES)})"
    return f"{word().title()}'s {rng.choice(['Family', 'Bar', 'Point'])}"


def street():
    roll = rng.random()
    if roll < 0.5:
        return f"RR. {rng.choice(FIRST)} {surname()}"
    if roll < 0.75:
        return f"Rruga {rng.choice(FIRST).title()} {surname().title()}"
    if roll < 0.9:
        return f"RR. {rng.choice(STREET_NAMES).upper()}"
    return f"Rruga {rng.choice(STREET_NAMES)}"


def client_address():
    roll = rng.random()
    if roll < 0.62:
        a = f"{street()}, TIRANE"
    elif roll < 0.8:
        a = f"{rng.choice(AREAS)}, TIRANE"
    elif roll < 0.86:
        a = f"{rng.choice(AREAS)}, {rng.choice(['KAMEZ', 'KASHAR', 'VAQARR'])}, TIRANE"
    elif roll < 0.89:
        a = f"{street()}, {rng.choice(['KATI 1', 'KATI I PARE', 'PALLATI 3'])}, TIRANE"
    elif roll < 0.905:
        a = f"{street()}, DURRES"
    elif roll < 0.91:
        a = f"{rng.choice(AREAS)}, {rng.choice(['KORCE', 'KRUJE', 'ELBASAN'])}"
    elif roll < 0.95:
        a = f'Rruga "{rng.choice(FIRST).title()} {surname().title()}", TIRANE'
    elif roll < 0.965:
        a = f"{street()}, {rng.choice(['KAMEZ. TIRANE', 'TIRANE.', 'TIRNAE', 'Tirane'])}"
    elif roll < 0.98:
        a = rng.choice(AREAS)
    else:
        a = f"{street()},TIRANE"
    return a


def nipt(letters="JKLM"):
    while True:
        n = rng.choice(letters) + f"{rng.randrange(10**8):08d}" + rng.choice("ABCDEFGHIJKLMNOPRSTUVZ")
        if n not in used_nipts and not n.startswith("K471"):
            used_nipts.add(n)
            return n


def pad_nipt(n, kind):
    return {"trail": n + " ", "lead": " " + n, "trail3": n + "   ", "trail2": n + "  ", "both": " " + n + " "}[kind]


def pad_address(a):
    return " " + a if rng.random() < 0.3 else a + rng.choice([" ", "  "])


def klientet():
    used_names.update({"*KLIENTE TE NDRYSHEM", "3 YJE", "ANULLUAR"})
    accounts = rng.sample(range(412100, 413000), 741) + rng.sample(range(4114000, 4120000), 2190)
    accounts = [a for a in accounts if a != 412732]
    rng.shuffle(accounts)
    planted_nipt = "K47120593B"
    used_nipts.add(planted_nipt)
    rows = []
    for _ in range(2931 - 3):
        rows.append([None, unique(client_name), client_address(), nipt()])
    # Quirks of the source export.
    idx = rng.sample(range(len(rows)), 600)
    cursor = iter(idx)
    for kind, n in [("trail", 87), ("lead", 32), ("trail3", 5), ("trail2", 2), ("both", 1)]:
        for _ in range(n):
            r = rows[next(cursor)]
            r[3] = pad_nipt(r[3], kind)
    for _ in range(255):
        r = rows[next(cursor)]
        r[2] = pad_address(r[2])
    for _ in range(3):
        rows[next(cursor)][2] = ""
    rows[next(cursor)][3] = ""
    odd = rows[next(cursor)]
    odd[3] = odd[3][:5] + odd[3][6:]  # one digit short
    odd = rows[next(cursor)]
    odd[3] = odd[3][:9] + "7" + odd[3][9:]  # one digit too many
    r = rows[next(cursor)]
    r[2] = f'Rruga "{rng.choice(FIRST).title()} {surname().title()}, TIRANE'  # unbalanced quote
    r = rows[next(cursor)]
    r[1] = f"{r[1]}, SHPK"  # a quoted name
    used_names.add(r[1].upper())
    r = rows[next(cursor)]
    r[1] = f'{r[1]} "{rng.choice(NOTES)}"'
    used_names.add(r[1].upper())
    # Five NIPTs shared by two clients each (distinct Llogari).
    shared = ["M02317046H", "L71208033S", "K90415021P", "M11604052E", "L30927018C"]
    for s in shared:
        a, b = rows[next(cursor)], rows[next(cursor)]
        a[3], b[3] = s, s
        used_nipts.add(s)
    rows.sort(key=lambda r: r[1].upper())
    special = [
        ["411001", "*KLIENTE TE NDRYSHEM", "", ""],
        ["412732", "3 YJE", "LIVADHET, KAMEZ, TIRANE", planted_nipt + " "],
    ]
    rows.insert(len(rows) * 2 // 3, ["", "ANULLUAR", " ", " "])
    for r, acc in zip(rows, accounts):
        r[0] = str(acc)
    rows = special + rows
    padded = sum(1 for r in rows if r[3] != r[3].strip())
    assert len(rows) == 2931 and padded == 129, (len(rows), padded)
    assert len({r[0] for r in rows}) == 2931
    write("KLIENTET.csv", ["Llogari", "Emër", "Adresa", "NIPT"], rows)


def supplier_name():
    roll = rng.random()
    w = word()
    if roll < 0.3:
        return f"{w} SHPK"
    if roll < 0.42:
        return f"{w} DISTRIBUTION"
    if roll < 0.5:
        return f"{w} GROUP"
    if roll < 0.57:
        return f"{w[0]} & {word()[0]} {rng.choice(['TREGTI', 'IMPORT', 'COMPANY'])}"
    if roll < 0.64:
        return " ".join(rng.choice("ABDEFGHKLMNPRSTVZ") for _ in range(rng.randint(3, 5)))
    if roll < 0.72:
        return f"{w.title()} {rng.choice(['Group', 'Distribution', 'Sh.p.k', 'Company'])}"
    if roll < 0.78:
        return f"{w}-{rng.randint(1, 99):02d}"
    return person()


def supplier_address():
    roll = rng.random()
    if roll < 0.35:
        return ""
    if roll < 0.75:
        return f"{street()}, TIRANE"
    if roll < 0.82:
        return f"{street()}, DURRES"
    if roll < 0.9:
        return f"Autostrada Tirane-Durres, Km {rng.randint(1, 30)}, TIRANE"
    if roll < 0.95:
        return street()
    return f"{rng.choice(AREAS)}, {rng.choice(['KASHAR', 'VORE', 'FUSHE KRUJE'])}, TIRANE"


def furnitoret():
    used_names.clear()
    used_names.update({"A & D GRUP", "A & V DISTRIBUTION", "FURNITORE TE NDRYSHEM"})
    rows = []
    while len(rows) < 186 - 3:
        name = unique(supplier_name)
        if name.upper() < "A & W":
            used_names.discard(name.upper())
            continue
        rows.append([name, supplier_address(), nipt("JKLM")])
    idx = iter(rng.sample(range(len(rows)), 60))
    for _ in range(6):
        rows[next(idx)][2] = ""
    for kind in ["trail", "trail", "lead"]:
        r = rows[next(idx)]
        r[2] = pad_nipt(r[2], kind)
    for _ in range(18):
        r = rows[next(idx)]
        if r[1]:
            r[1] = pad_address(r[1])
    r = rows[next(idx)]
    r[1] = f'Rruga  "{surname().title()}", Pallati  "{word().title()}  Residence", Hyrja   Nr.  2,  Kati  Nr. 1, Zyra Nr. 4  '
    r = rows[next(idx)]
    r[1] = f'Rruga "{rng.choice(FIRST).title()} {surname().title()}", TIRANE'
    rows.append(["FURNITORE TE NDRYSHEM", "", ""])
    rows.sort(key=lambda r: r[0].upper())
    rows = [
        ["A & D GRUP", "Rr.Ullishtes ", "L60318042D"],
        ["A & V DISTRIBUTION", "KASHAR, TIRANE", "M30512077R"],
    ] + rows
    used = [r[2].strip() for r in rows if r[2].strip()]
    assert len(rows) == 186 and len(used) == len(set(used)), len(rows)
    write("FURNITORET.csv", ["Emër", "Adresa", "NIPT"], rows)


if __name__ == "__main__":
    artikuj()
    klientet()
    furnitoret()
