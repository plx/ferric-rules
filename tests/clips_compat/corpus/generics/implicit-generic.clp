;; A defmethod can introduce its generic without a preceding defgeneric.
;; Level: basic
;; Covers: generics, implicit-generic
(defmethod twice ((?x INTEGER)) (* ?x 2))
(defrule probe => (printout t (twice 6) crlf))
