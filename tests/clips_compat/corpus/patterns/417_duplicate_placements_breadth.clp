(deffacts d (first a) (second b) (lst a a b b))
(defrule observe (first ?a) (second ?b) (lst $?before ?a $?middle ?b $?after)
 => (printout t (length$ ?before) ":" (length$ ?middle) ":" (length$ ?after) " " ?before "|" ?middle "|" ?after crlf))
