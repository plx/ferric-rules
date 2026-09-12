;; Only symbol FALSE is false; zero and strings are true.
;; Level: basic
;; Covers: and, not, or
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (not FALSE) " " (not 0) " " (not "FALSE") " " (and 0 TRUE) " " (or FALSE 0) crlf))
