(defglobal ?*count* = 0)
(deffacts d (first a) (second b) (lst b a) (lst missing b) (lst a missing) (lst))
(defrule unexpected (first ?a) (second ?b) (lst $? ?a $? ?b $?) => (bind ?*count* (+ ?*count* 1)))
(defrule done (declare (salience -10)) => (printout t "count " ?*count* crlf))
