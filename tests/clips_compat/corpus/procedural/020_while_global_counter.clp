;; While reevaluates the condition after each complete body execution.
;; Level: interaction
;; Covers: -, >, bind, defglobal, while
;; Run with load, reset, and run in a fresh environment.

(defglobal ?*count* = 3)

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (while (> ?*count* 0) do
        (printout t ?*count* crlf)
        (bind ?*count* (- ?*count* 1)))
    (printout t "end=" ?*count* crlf))
