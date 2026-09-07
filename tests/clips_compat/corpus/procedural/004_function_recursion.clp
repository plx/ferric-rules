;; Recursive calls preserve each invocation parameter binding.
;; Level: interaction
;; Covers: *, -, =, deffunction, factorial, if
;; Run with load, reset, and run in a fresh environment.

(deffunction factorial (?n) (if (= ?n 0) then 1 else (* ?n (factorial (- ?n 1)))))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (factorial 0) " " (factorial 5) crlf))
