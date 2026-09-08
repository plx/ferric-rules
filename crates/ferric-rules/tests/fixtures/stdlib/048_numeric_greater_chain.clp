;; Greater-than accepts a strictly descending sequence.
;; Level: boundary
;; Covers: >
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (> 3 2 1) " " (> 3 2 2) crlf))
