;; Format respects the numeric zero-padding flag.
;; Level: boundary
;; Covers: format
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (format nil "%04d" 7) crlf))
