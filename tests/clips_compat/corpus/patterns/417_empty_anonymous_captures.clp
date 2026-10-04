(deffacts d (first a) (second b) (lst adjacent a b) (lst edges a x b))
(defrule observe (first ?a) (second ?b) (lst ?id $? ?a $?middle ?b $?)
 => (printout t ?id ":" (length$ ?middle) ":" ?middle crlf))
