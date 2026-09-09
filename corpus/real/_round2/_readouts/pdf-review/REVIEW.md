# Word PDF Review

Reviewed PDF exports from `C:/word/real-word-round2-20260907/_previews/edited`.
Full-page renders and header crops are stored here, outside the delivery tree.
The delivery `_previews` directory was not modified.

## Headers

| PDF | Page | Visible header | Appearance |
| --- | --- | --- | --- |
| hf-variants--header.pdf | 1 | U+9996 U+9875, space, U+9875 U+7709 | Centered, horizontal rule |
| hf-variants--header.pdf | 2 | U+5076 U+6570 U+9875, space, U+9875 U+7709 | Centered, horizontal rule |
| hf-variants--header.pdf | 3 | rsword, space, U+9875 U+7709 | Left aligned, no horizontal rule |
| sections-three--header.pdf | 1 | U+7B2C U+4E00 U+8282 U+9875 U+7709 | Centered, horizontal rule |
| sections-three--header.pdf | 2 | U+7B2C U+4E8C U+8282 U+72EC U+7ACB U+9875 U+7709 | Centered, horizontal rule; landscape page |
| sections-three--header.pdf | 3 | rsword, space, U+9875 U+7709 | Left aligned, no horizontal rule |
| strict-basic-2--header.pdf | 1 | rsword, space, U+9875 U+7709 | Left aligned, no horizontal rule |

All seven headers are visible without body-text overlap. The exact Unicode text,
text coordinates, page sizes, and render paths are in `header-pdf-review.json`.
Header crop names follow `<pdf-stem>-page-<n>-header.png`.

## Canvas New Image

`canvas-floating--newimage.pdf` has one page. The new bright green image is visible
only as a narrow horizontal stripe above the white canvas. The canvas's blue
rectangle, orange ellipse, blue line, and arrow are visible.

Coordinates below use PDF points and a top-left origin:

| Object | Left | Top | Right | Bottom |
| --- | ---: | ---: | ---: | ---: |
| New green image, Image21 | 397.3 | 103.2 | 505.3 | 157.2 |
| White canvas rectangle | 155.0 | 118.8 | 515.0 | 318.8 |

The white canvas spans the image's full width and the lower 38.4 of its 54 pt
height, covering about 71.1% of its area. The visible top stripe is 15.6 pt high,
about 28.9% of the image height. These are PDF object bounds; raster edge pixels
can vary slightly with antialiasing.

The decoded page content establishes the paint order: Image21 is drawn first,
then a white filled rectangle is drawn over it. The PDF content below uses the
PDF bottom-left coordinate system (page height 841.92 pt):

```text
/P <</MCID 10>> BDC q
397.3 684.72 108 54 re
W* n
108 0 0 54 397.3 684.72 cm
/Image21 Do Q
EMC /P <</MCID 11>> BDC 1 g
155 523.12 360 200 re
f*
```

Thus the rendered export demonstrates actual coverage by the white canvas,
not merely coincident object bounds. Screenshot evidence is
`canvas-floating--newimage-page-1.png`.

The review used `pypdfium2` for rasterization and `pdfplumber`/`pdfminer` for PDF
objects, text, and decoded content. It did not open, modify, or save any Word file.
