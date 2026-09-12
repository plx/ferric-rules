;; Loop-for-count skips a range whose start exceeds its end.
;; Level: boundary
;; Covers: loop-for-count
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (loop-for-count (?i 3 2) do (printout t "wrong" crlf))
    (printout t "after" crlf))
