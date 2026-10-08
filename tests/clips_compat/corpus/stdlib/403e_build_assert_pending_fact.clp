;; An earlier fact's field cannot redefine the template or ordered relation
;; of a later fact in the same assert: build fails and the facts keep them.
(deftemplate p (slot x))
(defrule r =>
  (assert (trigger (build "(deftemplate p (slot y))")) (p (x 42)))
  (assert (trigger (build "(deftemplate q (slot a))")) (q 1))
  (printout t (deftemplate-slot-names p) " " (deftemplate-slot-names q) crlf))
(defrule show-trigger (trigger ?t) => (printout t "trigger " ?t crlf))
(defrule show-p (p (x ?x)) => (printout t "p " ?x crlf))
(defrule show-q (q ?v) => (printout t "q " ?v crlf))
