;; Substring uses one-based inclusive indices.
;; Level: basic
;; Covers: sub-string
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sub-string 2 4 "abcde") crlf))
