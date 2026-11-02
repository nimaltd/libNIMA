// The logo on the welcome page, as terNIMA draws its own: a small "lib" in teal, then "NIMA" in big orange block letters, both
// with a darker shadow, on six lines of text. terNIMA's terminal draws these characters as shapes rather than from a font, and a
// page cannot count on a font to have them, so here they are shapes too. It has no imports, so its tests run in Node as they are.

/** The size of one character of the logo, in pixels of the picture. */
export const CELL_WIDTH = 9;
export const CELL_HEIGHT = 18;

/** "lib" as pixels: two pixel rows make one line of text. 'X' is a pixel. Its shadow, a pixel down and to the right, is worked out as it is drawn. */
export const LIB = [
  "X..X..X....",
  "X.....X....",
  "X..X..XXXX.",
  "X..X..X...X",
  "X..X..X...X",
  ".X.X..XXXX.",
];

/** The pixel row of the logo where "lib" starts, so it stands on the same line as "NIMA", whose letters end on the fifth line of text. */
export const LIB_TOP = 4;

/** The columns "lib" takes, its shadow and a space before "NIMA" included. */
export const LIB_WIDTH = 13;

/** "NIMA" in block letters with a shadow edge: one entry per letter, six rows each. `█` is the face of a letter and the line characters are its shadow. */
export const NIMA = [
  ["███╗   ██╗", "████╗  ██║", "██╔██╗ ██║", "██║╚██╗██║", "██║ ╚████║", "╚═╝  ╚═══╝"],
  ["██╗", "██║", "██║", "██║", "██║", "╚═╝"],
  ["███╗   ███╗", "████╗ ████║", "██╔████╔██║", "██║╚██╔╝██║", "██║ ╚═╝ ██║", "╚═╝     ╚═╝"],
  [" █████╗ ", "██╔══██╗", "███████║", "██╔══██║", "██║  ██║", "╚═╝  ╚═╝"],
];

/** Where the two strokes of a double line run across a character. */
const LEFT = 3.5;
const RIGHT = 6.5;
const UPPER = 7.5;
const LOWER = 10.5;

/** Each line character as the strokes that make it, in one character's own pixels. */
export const LINES: Record<string, string> = {
  "═": `M0 ${UPPER}H${CELL_WIDTH}M0 ${LOWER}H${CELL_WIDTH}`,
  "║": `M${LEFT} 0V${CELL_HEIGHT}M${RIGHT} 0V${CELL_HEIGHT}`,
  "╗": `M0 ${UPPER}H${RIGHT}V${CELL_HEIGHT}M0 ${LOWER}H${LEFT}V${CELL_HEIGHT}`,
  "╔": `M${CELL_WIDTH} ${UPPER}H${LEFT}V${CELL_HEIGHT}M${CELL_WIDTH} ${LOWER}H${RIGHT}V${CELL_HEIGHT}`,
  "╝": `M0 ${LOWER}H${RIGHT}V0M0 ${UPPER}H${LEFT}V0`,
  "╚": `M${CELL_WIDTH} ${LOWER}H${LEFT}V0M${CELL_WIDTH} ${UPPER}H${RIGHT}V0`,
};

/** A line character's strokes moved to where it stands. */
function moved(strokes: string, x: number, y: number): string {
  return strokes.replace(/([MHV])(-?[\d.]+)(?: (-?[\d.]+))?/g, (_, command: string, first: string, second?: string) => {
    if (command === "M") {
      return `M${Number(first) + x} ${Number(second) + y}`;
    }
    return `${command}${Number(first) + (command === "H" ? x : y)}`;
  });
}

function box(x: number, y: number, width: number, height: number): string {
  return `M${x} ${y}h${width}v${height}h${-width}z`;
}

export type Logo = {
  width: number;
  height: number;
  libFace: string;
  libShadow: string;
  nimaFace: string;
  nimaShadow: string;
};

/** The logo as paths for a picture: what is filled, and for the shadow of "NIMA", what is stroked. */
export function logo(): Logo {
  const pixel = CELL_HEIGHT / 2;
  const isFace = (row: number, col: number) => LIB[row]?.[col] === "X";
  let libFace = "";
  let libShadow = "";

  // Every shadow falls on a pixel beside the letters, or on the row below them.
  for (let row = 0; row <= LIB.length; row++) {
    for (let col = 0; col < LIB_WIDTH; col++) {
      const x = col * CELL_WIDTH;
      const y = (LIB_TOP + row) * pixel;
      if (isFace(row, col)) {
        libFace += box(x, y, CELL_WIDTH, pixel);
      } else if (isFace(row - 1, col - 1)) {
        libShadow += box(x, y, CELL_WIDTH, pixel);
      }
    }
  }

  let nimaFace = "";
  let nimaShadow = "";
  let column = LIB_WIDTH;

  for (const letter of NIMA) {
    letter.forEach((row, line) => {
      [...row].forEach((character, at) => {
        const x = (column + at) * CELL_WIDTH;
        const y = line * CELL_HEIGHT;
        if (character === "█") {
          nimaFace += box(x, y, CELL_WIDTH, CELL_HEIGHT);
        } else if (character !== " ") {
          nimaShadow += moved(LINES[character], x, y);
        }
      });
    });
    column += [...letter[0]].length;
  }

  return { width: column * CELL_WIDTH, height: NIMA[0].length * CELL_HEIGHT, libFace, libShadow, nimaFace, nimaShadow };
}
