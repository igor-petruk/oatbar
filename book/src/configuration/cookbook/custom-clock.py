import datetime
import cairo
import os
import time
import sys

while True:
    WIDTH, HEIGHT = 270, 28
    sfc = cairo.ImageSurface(cairo.Format.ARGB32, WIDTH, HEIGHT)
    ctx = cairo.Context(sfc)
    ctx.set_font_size(16) 
    ctx.select_font_face("Courier", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL) 
    ctx.move_to(3, 20)
    ctx.set_source_rgb(1.0, 1.0, 1.0)
    time_str="%s" % datetime.datetime.now()
    ctx.show_text(time_str)
    ctx.fill()

    output_filename = '/tmp/custom-clock.png'
    sfc.write_to_png(output_filename)

    print(output_filename)
    print(time_str)
    sys.stdout.flush()
    time.sleep(1)

