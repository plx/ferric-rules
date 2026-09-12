;; Format nil returns a formatted STRING without router output.
;; Level: basic
;; Covers: format
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (format nil "n=%d value=%.2f %% %s" 7 2.5 done) crlf))
