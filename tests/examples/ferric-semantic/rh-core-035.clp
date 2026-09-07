; RH-CORE-035: candidate facts select at most one deterministic launch action for a session.
(deffacts launch
  (session session-42)
  (candidate session-42 promotion)
  (candidate session-42 sign-in)
  (candidate session-42 welcome))
(defrule sign-in (declare (salience 30)) (session ?s) (candidate ?s sign-in) (not (action ?s ?kind)) => (assert (action ?s sign-in)) (printout t "action " ?s " sign-in" crlf))
(defrule welcome (declare (salience 20)) (session ?s) (candidate ?s welcome) (not (action ?s ?kind)) => (assert (action ?s welcome)) (printout t "incorrect welcome" crlf))
(defrule promotion (declare (salience 10)) (session ?s) (candidate ?s promotion) (not (action ?s ?kind)) => (assert (action ?s promotion)) (printout t "incorrect promotion" crlf))
