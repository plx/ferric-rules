;; Issue #323: a call with no applicable method does not run the body.
(defgeneric guarded)
(defmethod guarded (?x (?n INTEGER))
  (printout t "body" crlf))
(defrule probe => (guarded blue green) (printout t "after" crlf))
