;; Trigonometric functions at zero.
;; Level: boundary
;; Covers: cos, sin, tan
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sin 0) " " (cos 0) " " (tan 0) crlf))
