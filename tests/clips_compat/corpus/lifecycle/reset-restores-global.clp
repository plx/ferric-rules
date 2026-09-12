;; Level: interaction
;; Covers: reset, defglobal
;; Resets: 2
(defglobal ?*count* = 0)
(defrule probe => (printout t ?*count* crlf) (bind ?*count* (+ ?*count* 1)))
