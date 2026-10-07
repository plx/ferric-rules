; call-specific-method on a method whose query fails halts the rule.
(deffunction note (?x) (printout t "q" ?x crlf) TRUE)
(defgeneric g)
(defmethod g 1 ((?a (note a)) (?b INTEGER (note b))) (create$ m1 ?a ?b))
(defmethod g 3 (?a ?b) (create$ m3 ?a ?b))
(defrule r1 => (printout t "before" crlf) (printout t (call-specific-method g 1 1 x) crlf) (printout t "after" crlf))
