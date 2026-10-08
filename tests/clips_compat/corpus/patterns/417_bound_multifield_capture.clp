; Separate phases expose all empty and nonempty bound captures independently.
(deffacts d (key empty) (key pair a b) (lst a b a b) (phase empty))
(defrule observe (key ?id $?wanted) (lst $?before $?wanted $?after) (phase ?id)
 => (printout t ?id ":" (length$ ?before) ":" (length$ ?wanted) ":" (length$ ?after) crlf))
(defrule next-pair (declare (salience -10)) ?phase <- (phase empty)
 => (retract ?phase) (assert (phase pair)))
