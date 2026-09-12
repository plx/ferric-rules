;; Angle conversions are checked by rounding round-trip results.
;; Level: basic
;; Covers: deg-grad, deg-rad, grad-deg, pi, rad-deg, round
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (round (rad-deg (pi))) " " (round (rad-deg (deg-rad 45))) " " (round (deg-grad 90)) " " (round (grad-deg 100)) crlf))
