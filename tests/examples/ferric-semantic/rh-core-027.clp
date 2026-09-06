; RH-CORE-027: RHS template assertions flatten multifield values into the declared multislot.
(deftemplate packet (slot id (type INTEGER)) (multislot items))
(deffacts seed (source 12 red "blue" 3.5))
(defrule create-packet (source ?id $?items) => (assert (packet (id ?id) (items $?items))))
(defrule read-packet (packet (id ?id) (items $?items)) => (printout t ?id " " (length$ ?items) crlf) (assert (result ?id $?items)))
