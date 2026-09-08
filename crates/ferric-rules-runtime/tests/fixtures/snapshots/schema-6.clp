(defglobal ?*captured* = FALSE)
(deftemplate named (slot name (type INSTANCE-NAME)))
(deffacts names (named (name [seed])))
(defrule first
  (declare (salience 10))
  (named (name ?name))
  => (printout t (instance-namep ?name) crlf))
(defrule capture
  (payload ?text ?symbol ?name)
  =>
  (bind ?*captured* (create$ ?text ?symbol ?name))
  (printout t ?text "|" ?symbol "|" ?name crlf))
