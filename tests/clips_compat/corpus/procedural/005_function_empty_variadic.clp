;; A variadic parameter collects zero remaining arguments into an empty multifield.
;; Level: boundary
;; Covers: count-tail, deffunction, length$
;; Run with load, reset, and run in a fresh environment.

(deffunction count-tail (?first $?rest) (length$ ?rest))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (count-tail head) crlf))
