(defrule action-task-connect-receiver-of-sim
  "Enable peer connection to the simulator"
  (confval (path "/rcll-simulator/enabled") (value TRUE))
  (confval (path "/rcll-simulator/host") (value ?peer-address))
  (confval (path "/rcll-simulator/robot-recv-ports") (is-list TRUE) (list-value $?recv-ports))
  (confval (path "/rcll-simulator/robot-send-ports") (is-list TRUE) (list-value $?send-ports))
  (not (protobuf-peer (name robot1)))
  (not (executive-finalize))
  =>
  (printout info "Enabling robot simulation peers" crlf)
  (if (<> (length$ ?recv-ports) (length$ ?send-ports)) then
    (printout error "Expected number or recv ports to be equal to send ports for simulator robots (" (length$ ?recv-ports) " != "(length$ ?send-ports) ")" crlf)
   else
    (loop-for-count (?i (length$ ?recv-ports)) do
      (bind ?peer-id (pb-peer-create-local ?peer-address (string-to-field (nth$ ?i ?send-ports)) (string-to-field (nth$ ?i ?recv-ports))))
      (assert (protobuf-peer (name (sym-cat "ROBOT" ?i)) (peer-id ?peer-id)))
    )
  )
)
