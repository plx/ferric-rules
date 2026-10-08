; Replacement arguments re-run each parameter's type check, then its query,
; left to right, before the next applicable method is chosen.
(defglobal ?*log* = "")
(deffunction note (?x) (bind ?*log* (str-cat ?*log* ?x)) TRUE)
(defgeneric g)
(defmethod g 1 ((?a (note a)) (?b INTEGER (note b))) (create$ m1 ?a ?b))
(defmethod g 2 ((?a INTEGER (note c)) (?b (note d))) (create$ m2 (override-next-method ?a x)))
(defmethod g 3 (?a ?b) (create$ m3 ?a ?b))
(defrule r => (printout t (g 1 2) "|" ?*log* crlf))
