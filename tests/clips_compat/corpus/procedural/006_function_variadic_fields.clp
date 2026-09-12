;; A variadic parameter preserves remaining argument order and types.
;; Level: interaction
;; Covers: deffunction, tail
;; Run with load, reset, and run in a fresh environment.

(deffunction tail (?first $?rest) ?rest)

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (length$ (tail head a 2 "c")) " " (nth$ 1 (tail head a 2 "c")) " " (stringp (nth$ 3 (tail head a 2 "c"))) crlf))
