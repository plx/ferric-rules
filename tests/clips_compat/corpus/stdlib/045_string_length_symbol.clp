;; Str-length accepts a SYMBOL argument.
;; Level: boundary
;; Covers: str-length
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (str-length abc) crlf))
