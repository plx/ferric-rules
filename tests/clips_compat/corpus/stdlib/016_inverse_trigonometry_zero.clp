;; Inverse trigonometric functions at exact zero-valued reference points.
;; Level: boundary
;; Covers: acos, asin, atan
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (asin 0) " " (acos 1) " " (atan 0) crlf))
