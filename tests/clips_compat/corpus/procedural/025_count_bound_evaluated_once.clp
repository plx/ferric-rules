;; Loop-for-count evaluates its end bound once before iteration.
;; Level: boundary
;; Covers: bind, defglobal, loop-for-count
;; Run with load, reset, and run in a fresh environment.

(defglobal ?*end* = 3)

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (loop-for-count (?i 1 ?*end*) do
        (printout t ?i crlf)
        (bind ?*end* 0)))
