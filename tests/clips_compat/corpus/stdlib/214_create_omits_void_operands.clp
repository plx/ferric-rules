;; create$ runs a VOID operand for its effects and leaves it out of the
;; result; empty STRINGs and nested multifields stay.
;; Level: boundary
;; Covers: create$, length$, printout
(defglobal ?*trace* = 0)
(deffunction emit () (printout t "callable:" (bind ?*trace* (+ ?*trace* 1)) crlf))
(defrule probe =>
  (bind ?r (create$ a (printout t "direct" crlf) "b c"))
  (printout t (length$ ?r) " " ?r crlf)
  (bind ?r (create$ a (emit) "b c"))
  (printout t (length$ ?r) " " ?r crlf)
  (bind ?r (create$ (printout t "direct" crlf) a "b c" (emit)))
  (printout t (length$ ?r) " " ?r crlf)
  (bind ?r (create$ (printout t "direct" crlf) (emit)))
  (printout t (length$ ?r) " " ?r crlf)
  (bind ?r (create$ before (create$ (printout t "direct" crlf) "b c" (create$ (emit) 7)) (printout t "direct" crlf) after))
  (printout t (length$ ?r) " " ?r crlf)
  (bind ?r (create$ (emit) "" (create$ (printout t "direct" crlf)) ""))
  (printout t (length$ ?r) " " ?r crlf))
