(defglobal ?*session-count* = 0)

(defrule count-session
    (session-start)
    =>
    (bind ?*session-count* (+ ?*session-count* 1))
    (printout t "session " ?*session-count* crlf))
