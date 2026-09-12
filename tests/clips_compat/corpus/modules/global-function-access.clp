;; A deffunction reads and updates its defining module global.
;; Level: basic
;; Covers: modules, global-function-access
(defglobal ?*count* = 2)
(deffunction advance () (bind ?*count* (+ ?*count* 1)) ?*count*)
(defrule probe => (printout t (advance) ":" ?*count* crlf))
