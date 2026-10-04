(deftemplate item (slot v))
(deftemplate holder (slot ref (type FACT-ADDRESS)) (multislot refs (type FACT-ADDRESS)))
(deffacts seed (item (v 1)) (item (v 2)))
(defrule store (declare (salience 10)) ?a <- (item (v 1)) ?b <- (item (v 2)) =>
  (assert (holder (ref ?a) (refs ?a ?b)))
  (assert (links ?a ?b)))
(defrule inspect (holder (ref ?a) (refs $?refs)) (links ?same ?b) =>
  (printout t ?a ":" ?refs ":" (eq ?a ?same) ":" (eq ?b (nth$ 2 ?refs)) crlf)
  (printout t (create$ before ?refs after) crlf))
