(deffacts d (first a) (second b) (lst a x x b) (lst a x y b) (lst a a b))
; Salience separates rules while retaining each rule's exact split order.
(defrule repeated (declare (salience 10)) (first ?a) (second ?b) (lst $?pre ?a $?mid ?x ?x $?tail ?b $?)
 => (printout t "repeat " (length$ ?pre) ":" (length$ ?mid) ":" ?x ":" (length$ ?tail) crlf))
(defrule alternative (first ?a) (second ?b) (lst $?pre ?a|x $?mid ?b $?)
 => (printout t "or " (length$ ?pre) ":" (length$ ?mid) crlf))
