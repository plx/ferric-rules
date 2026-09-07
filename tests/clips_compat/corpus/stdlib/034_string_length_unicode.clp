;; UTF-8 string length distinguishes code points from encoded bytes.
;; Level: basic
;; Covers: str-length
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (str-length "café") " " (str-length "猫") crlf))
